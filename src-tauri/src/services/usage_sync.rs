//! 跨设备用量同步（v12+）
//!
//! 与配置同步（`s3_sync` / `webdav_sync`）并行的独立通道，专门同步 `usage_daily_rollups`。
//!
//! ## 模型：中心合并库 + 增量推送
//!
//! 每台设备把**自己的** rollups 推到远端独立 slot，永不覆盖他人：
//! ```
//! {remote_root}/usage/v1/_index.json                 # 设备注册表
//! {remote_root}/usage/v1/{device_id}/rollups.sql     # 该设备全量汇总
//! {remote_root}/usage/v1/{device_id}/manifest.json   # 元数据（hash/size/时间）
//! ```
//!
//! 拉取时读 `_index.json` 获取所有 device_id，逐个下载 rollups.sql，
//! 按 `(date, app_type, provider_id, model, request_model, pricing_model, device_id)` 主键
//! `INSERT OR REPLACE` 合并到本地——主键含 device_id 保证不同设备不冲突。
//!
//! ## 与配置同步的区别
//! - 配置同步：覆盖语义（last-write-wins），全量 db.sql + skills.zip
//! - 用量同步：合并语义（append/sum），仅 rollups.sql，每设备独立 slot
//!
//! 明细（proxy_request_logs）不跨设备同步，保持本地 30 天。

use crate::database::Database;
use crate::error::AppError;
use crate::services::s3;
use crate::settings::{self, S3SyncSettings};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use tokio::sync::Mutex;

/// 用量同步协议版本
const USAGE_PROTOCOL_VERSION: u32 = 1;
/// 远端路径前缀中的版本段
const USAGE_PATH_VERSION: &str = "v1";
/// rollups.sql 大小上限（512 MiB，与配置同步一致）
const MAX_USAGE_ARTIFACT_BYTES: usize = 512 * 1024 * 1024;

/// 远端 rollups.sql 的对象 key
fn rollups_key(remote_root: &str, device_id: &str) -> String {
    format!("{remote_root}/usage/{USAGE_PATH_VERSION}/{device_id}/rollups.sql")
}

/// 远端 manifest.json 的对象 key
fn manifest_key(remote_root: &str, device_id: &str) -> String {
    format!("{remote_root}/usage/{USAGE_PATH_VERSION}/{device_id}/manifest.json")
}

/// 设备注册表的对象 key（全局唯一，所有设备共用）
fn index_key(remote_root: &str) -> String {
    format!("{remote_root}/usage/{USAGE_PATH_VERSION}/_index.json")
}

/// 全局用量同步锁（与配置同步的 sync_mutex 独立，避免互相阻塞）
static USAGE_SYNC_MUTEX: OnceLock<Mutex<()>> = OnceLock::new();
fn usage_sync_mutex() -> &'static Mutex<()> {
    USAGE_SYNC_MUTEX.get_or_init(|| Mutex::new(()))
}

use std::sync::OnceLock;

/// 单个设备的用量快照 manifest
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct UsageManifest {
    /// 固定为 "cc-switch-usage-sync"
    format: String,
    /// 固定为 1
    version: u32,
    /// 产生该快照的设备 ID
    device_id: String,
    /// 产生该快照的设备名（展示用）
    device_name: String,
    /// 上传时间（RFC3339）
    created_at: String,
    /// rollups.sql 的 sha256
    rollups_sha256: String,
    /// rollups.sql 的字节大小
    rollups_size: u64,
}

/// 设备注册表条目
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceRegistryEntry {
    pub device_id: String,
    pub device_name: String,
    /// 最后一次上传时间（RFC3339）
    pub last_upload_at: String,
}

/// 设备注册表（_index.json）
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DeviceRegistry {
    /// device_id → entry
    devices: BTreeMap<String, DeviceRegistryEntry>,
}

impl DeviceRegistry {
    fn new() -> Self {
        Self {
            devices: BTreeMap::new(),
        }
    }
}

fn creds_for(settings: &S3SyncSettings) -> s3::S3Credentials {
    s3::S3Credentials {
        access_key_id: settings.access_key_id.clone(),
        secret_access_key: settings.secret_access_key.clone(),
        region: settings.region.clone(),
        bucket: settings.bucket.clone(),
        endpoint: settings.endpoint.clone(),
    }
}

fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    let result = hasher.finalize();
    result.iter().map(|b| format!("{b:02x}")).collect()
}

/// 导出本设备的 usage_daily_rollups 为 SQL 字符串。
///
/// 只导出属于本设备（device_id 匹配）的行——其他设备的行（通过下载合并进来的）
/// 不会被本设备回传，避免循环放大。
fn export_local_rollups_sql(db: &Database, device_id: &str) -> Result<Vec<u8>, AppError> {
    let conn = crate::database::lock_conn!(db.conn);
    // 导出本设备 rollups 的所有行为 INSERT OR IGNORE 语句（合并时按 PK 去重）。
    let mut stmt = conn
        .prepare(
            "SELECT date, app_type, provider_id, model, request_model, pricing_model, device_id,
                    request_count, success_count, input_tokens, output_tokens,
                    cache_read_tokens, cache_creation_tokens, total_cost_usd,
                    CAST(avg_latency_ms AS INTEGER) AS avg_latency_ms
             FROM usage_daily_rollups
             WHERE device_id = ?1 OR (device_id = '' AND ?2 = '')",
        )
        .map_err(|e| AppError::Database(e.to_string()))?;

    let rows = stmt
        .query_map(rusqlite::params![device_id, device_id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, String>(5)?,
                row.get::<_, String>(6)?,
                row.get::<_, i64>(7)?,
                row.get::<_, i64>(8)?,
                row.get::<_, i64>(9)?,
                row.get::<_, i64>(10)?,
                row.get::<_, i64>(11)?,
                row.get::<_, i64>(12)?,
                row.get::<_, String>(13)?,
                row.get::<_, i64>(14)?,
            ))
        })
        .map_err(|e| AppError::Database(e.to_string()))?;

    let mut sql = String::new();
    sql.push_str("BEGIN TRANSACTION;\n");
    for row_res in rows {
        let (
            date,
            app_type,
            provider_id,
            model,
            request_model,
            pricing_model,
            row_device_id,
            request_count,
            success_count,
            input_tokens,
            output_tokens,
            cache_read_tokens,
            cache_creation_tokens,
            total_cost_usd,
            avg_latency_ms,
        ) = row_res.map_err(|e| AppError::Database(e.to_string()))?;

        // 转义单引号
        let esc = |s: &str| s.replace('\'', "''");
        sql.push_str(&format!(
            "INSERT OR REPLACE INTO usage_daily_rollups \
             (date, app_type, provider_id, model, request_model, pricing_model, device_id, \
              request_count, success_count, input_tokens, output_tokens, \
              cache_read_tokens, cache_creation_tokens, total_cost_usd, avg_latency_ms) \
             VALUES ('{}','{}','{}','{}','{}','{}','{}',{},{},{},{},{},{},'{}',{});\n",
            esc(&date),
            esc(&app_type),
            esc(&provider_id),
            esc(&model),
            esc(&request_model),
            esc(&pricing_model),
            esc(&row_device_id),
            request_count,
            success_count,
            input_tokens,
            output_tokens,
            cache_read_tokens,
            cache_creation_tokens,
            esc(&total_cost_usd),
            avg_latency_ms,
        ));
    }
    sql.push_str("COMMIT;\n");
    Ok(sql.into_bytes())
}

/// 把远端 rollups.sql 合并到本地 usage_daily_rollups。
///
/// 使用 `INSERT OR REPLACE` 按 PK（含 device_id）覆盖；同设备重传不会重复，
/// 不同设备因 device_id 不同互不冲突。
fn merge_rollups_sql(db: &Database, sql_bytes: &[u8]) -> Result<(), AppError> {
    let sql_str =
        std::str::from_utf8(sql_bytes).map_err(|e| AppError::Database(format!("rollups.sql 非合法 UTF-8: {e}")))?;
    let conn = crate::database::lock_conn!(db.conn);
    conn.execute_batch(sql_str)
        .map_err(|e| AppError::Database(format!("合并 rollups.sql 失败: {e}")))?;
    Ok(())
}

/// 读取远端设备注册表（_index.json）。不存在时返回空表。
async fn fetch_device_registry(
    creds: &s3::S3Credentials,
    remote_root: &str,
) -> Result<DeviceRegistry, AppError> {
    let key = index_key(remote_root);
    match s3::get_object(creds, &key, MAX_USAGE_ARTIFACT_BYTES).await? {
        Some((bytes, _etag)) => {
            let registry: DeviceRegistry = serde_json::from_slice(&bytes).map_err(|e| {
                AppError::Database(format!("解析设备注册表失败: {e}"))
            })?;
            Ok(registry)
        }
        None => Ok(DeviceRegistry::new()),
    }
}

/// 写入远端设备注册表。
async fn put_device_registry(
    creds: &s3::S3Credentials,
    remote_root: &str,
    registry: &DeviceRegistry,
) -> Result<(), AppError> {
    let bytes = serde_json::to_vec_pretty(registry)
        .map_err(|e| AppError::Database(format!("序列化设备注册表失败: {e}")))?;
    s3::put_object(creds, &index_key(remote_root), bytes, "application/json").await
}

/// 上传本设备的 rollups 到远端独立 slot。
///
/// 步骤：
/// 1. 导出本设备 rollups.sql
/// 2. 上传 rollups.sql + manifest.json
/// 3. 更新 _index.json 注册本设备
pub async fn upload_usage(db: &Database, settings: &S3SyncSettings) -> Result<(), AppError> {
    let _guard = usage_sync_mutex().lock().await;
    let creds = creds_for(settings);
    let device_id = settings::device_id().to_string();

    // 1. 导出
    let rollups_bytes = export_local_rollups_sql(db, &device_id)?;
    let rollups_sha = sha256_hex(&rollups_bytes);
    let rollups_size = rollups_bytes.len() as u64;
    if rollups_size as usize > MAX_USAGE_ARTIFACT_BYTES {
        return Err(AppError::Database(format!(
            "rollups.sql 超过大小上限 ({} bytes)",
            MAX_USAGE_ARTIFACT_BYTES
        )));
    }

    // 2. 上传 artifacts（先 rollups 后 manifest，保证一致性）
    s3::put_object(
        &creds,
        &rollups_key(&settings.remote_root, &device_id),
        rollups_bytes,
        "application/sql",
    )
    .await?;

    let manifest = UsageManifest {
        format: "cc-switch-usage-sync".to_string(),
        version: USAGE_PROTOCOL_VERSION,
        device_id: device_id.clone(),
        device_name: crate::services::sync_protocol::detect_system_device_name()
            .unwrap_or_else(|| "Unknown Device".to_string()),
        created_at: chrono::Utc::now().to_rfc3339(),
        rollups_sha256: rollups_sha,
        rollups_size,
    };
    let manifest_bytes = serde_json::to_vec_pretty(&manifest)
        .map_err(|e| AppError::Database(format!("序列化 usage manifest 失败: {e}")))?;
    s3::put_object(
        &creds,
        &manifest_key(&settings.remote_root, &device_id),
        manifest_bytes,
        "application/json",
    )
    .await?;

    // 3. 更新设备注册表
    let mut registry = fetch_device_registry(&creds, &settings.remote_root).await?;
    registry.devices.insert(
        device_id.clone(),
        DeviceRegistryEntry {
            device_id,
            device_name: manifest.device_name.clone(),
            last_upload_at: manifest.created_at.clone(),
        },
    );
    put_device_registry(&creds, &settings.remote_root, &registry).await?;

    log::info!("用量同步：已上传本设备 rollups");
    Ok(())
}

/// 下载并合并所有设备的 rollups 到本地。
///
/// 本设备的 slot 也会被拉取并合并（幂等，INSERT OR REPLACE 覆盖相同行）。
/// 返回合并的设备数量。
pub async fn download_all_usage(
    db: &Database,
    settings: &S3SyncSettings,
) -> Result<usize, AppError> {
    let _guard = usage_sync_mutex().lock().await;
    let creds = creds_for(settings);

    // 1. 读设备注册表
    let registry = fetch_device_registry(&creds, &settings.remote_root).await?;
    if registry.devices.is_empty() {
        log::info!("用量同步：远端无设备注册表，跳过下载");
        return Ok(0);
    }

    // 2. 逐设备下载并合并
    let mut merged = 0usize;
    for (device_id, _entry) in &registry.devices {
        let key = rollups_key(&settings.remote_root, device_id);
        match s3::get_object(&creds, &key, MAX_USAGE_ARTIFACT_BYTES).await? {
            Some((bytes, _etag)) => {
                // 校验大小（hash 校验可选，manifest 单独拉取代价大，这里信任注册表）
                merge_rollups_sql(db, &bytes)?;
                merged += 1;
            }
            None => {
                log::warn!("用量同步：设备 {device_id} 的 rollups.sql 不存在，跳过");
            }
        }
    }

    log::info!("用量同步：已合并 {merged} 个设备的 rollups");
    Ok(merged)
}

/// 列出远端注册的所有设备（供前端展示设备筛选下拉）。
pub async fn list_remote_devices(
    settings: &S3SyncSettings,
) -> Result<Vec<DeviceRegistryEntry>, AppError> {
    let creds = creds_for(settings);
    let registry = fetch_device_registry(&creds, &settings.remote_root).await?;
    Ok(registry.devices.values().cloned().collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn export_local_rollups_sql_accepts_real_avg_latency() -> Result<(), AppError> {
        let db = Database::memory()?;
        {
            let conn = crate::database::lock_conn!(db.conn);
            conn.execute(
                "INSERT INTO usage_daily_rollups
                 (date, app_type, provider_id, model, request_model, pricing_model, device_id,
                  request_count, success_count, input_tokens, output_tokens,
                  cache_read_tokens, cache_creation_tokens, total_cost_usd, avg_latency_ms)
                 VALUES
                 ('2026-07-07', 'codex', 'p1', 'm1', '', '', 'device-a',
                  2, 2, 10, 20, 0, 0, '0.01', 12.5)",
                [],
            )
            .map_err(|e| AppError::Database(e.to_string()))?;
        }

        let sql = export_local_rollups_sql(&db, "device-a")?;
        let sql = String::from_utf8(sql)
            .map_err(|e| AppError::Database(format!("invalid utf8: {e}")))?;

        assert!(
            sql.contains("'2026-07-07','codex','p1','m1','','','device-a',2,2,10,20,0,0,'0.01',12"),
            "export should cast REAL avg_latency_ms to an integer literal, got: {sql}"
        );
        Ok(())
    }
}
