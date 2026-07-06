//! 跨设备用量同步命令（v12+）
//!
//! 复用 S3 同步凭证（与配置同步共用一个 S3 backend），但走独立的 usage/v1/ 远端路径。
//! 启用条件：S3 sync 已配置且 enabled。用量同步本身不单独配置凭证。

use crate::services::usage_sync::{self, DeviceRegistryEntry};
use crate::settings::{self, S3SyncSettings};
use crate::store::AppState;
use serde_json::{json, Value};
use tauri::State;

fn require_enabled_s3_settings() -> Result<S3SyncSettings, String> {
    let s = settings::get_s3_sync_settings().ok_or_else(|| {
        "S3 sync is not configured. Configure S3 sync first to enable usage sync.".to_string()
    })?;
    if !s.enabled {
        return Err("S3 sync is disabled. Enable it to use usage sync.".to_string());
    }
    Ok(s)
}

/// 手动上传本设备用量汇总到远端。
#[tauri::command]
pub async fn usage_sync_upload(
    state: State<'_, AppState>,
) -> Result<Value, String> {
    let db = state.db.clone();
    let settings = require_enabled_s3_settings()?;

    match usage_sync::upload_usage(&db, &settings).await {
        Ok(()) => Ok(json!({ "status": "uploaded" })),
        Err(e) => {
            log::error!("[UsageSync] upload failed: {e}");
            Err(e.to_string())
        }
    }
}

/// 拉取所有远端设备的用量并合并到本地。
#[tauri::command]
pub async fn usage_sync_download_all(
    state: State<'_, AppState>,
) -> Result<Value, String> {
    let db = state.db.clone();
    let settings = require_enabled_s3_settings()?;

    match usage_sync::download_all_usage(&db, &settings).await {
        Ok(merged) => {
            // 合并后通知前端刷新统计
            crate::usage_events::notify_log_recorded();
            Ok(json!({ "status": "downloaded", "mergedDevices": merged }))
        }
        Err(e) => {
            log::error!("[UsageSync] download failed: {e}");
            Err(e.to_string())
        }
    }
}

/// 列出远端注册的所有设备（用于前端设备筛选下拉的选项）。
#[tauri::command]
pub async fn usage_sync_fetch_devices() -> Result<Vec<DeviceRegistryEntry>, String> {
    let settings = require_enabled_s3_settings()?;
    usage_sync::list_remote_devices(&settings)
        .await
        .map_err(|e| {
            log::warn!("[UsageSync] fetch devices failed: {e}");
            e.to_string()
        })
}
