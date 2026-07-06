//! 跨设备用量自动同步 worker（v12+）
//!
//! 触发时机：
//! 1. **rollup 完成后**：`rollup_and_prune` 结束时调用 `notify_rollup_done()`（明细归档为汇总后，正是推送的好时机）
//! 2. **定时兜底**：每 30 分钟一次（rollup 不频繁，兜底保证跨设备拉取的新设备能尽快看到历史数据）
//!
//! 仅在 S3 sync `enabled && auto_sync` 时执行上传。下载需用户手动触发（避免后台静默改库）。
//!
//! 设计：复用配置同步的 S3 凭证与 auto_sync 开关，走独立的远端路径（usage/v1/）。

use crate::database::Database;
use crate::services::usage_sync;
use crate::settings;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc::{channel, Receiver, Sender};

/// 30 分钟兜底周期
const FALLBACK_INTERVAL_SECS: u64 = 30 * 60;
/// rollup 信号防抖（合并短时间内的多次 rollup）
const ROLLUP_DEBOUNCE_SECS: u64 = 5;

static ROLLUP_TX: std::sync::OnceLock<Sender<()>> = std::sync::OnceLock::new();

/// rollup 完成后调用，触发一次延迟上传。
pub fn notify_rollup_done() {
    let Some(tx) = ROLLUP_TX.get() else {
        return;
    };
    // try_send：通道满（容量 1）时直接丢弃，只保留「有变更」这一比特
    let _ = tx.try_send(());
}

/// 启动用量自动同步 worker。应在 app 启动时调用一次。
pub fn start_worker(db: Arc<Database>) {
    if ROLLUP_TX.get().is_some() {
        return;
    }
    let (tx, rx) = channel::<()>(1);
    if ROLLUP_TX.set(tx).is_err() {
        return;
    }
    tauri::async_runtime::spawn(async move {
        run_loop(db, rx).await;
    });
}

async fn run_loop(db: Arc<Database>, mut rx: Receiver<()>) {
    loop {
        // 先等一个信号（rollup 完成）或兜底超时
        let timeout = Duration::from_secs(FALLBACK_INTERVAL_SECS);
        let first = tokio::time::timeout(timeout, rx.recv()).await;

        let triggered = match first {
            Ok(Some(())) => true, // rollup 信号
            Ok(None) => return,   // 通道关闭，退出
            Err(_) => true,       // 兜底超时，也尝试一次（拉取场景：本设备无 rollup 但想看其他设备）
        };

        if !triggered {
            continue;
        }

        // rollup 信号防抖：短时间内的多次 rollup 合并为一次上传
        if ROLLUP_DEBOUNCE_SECS > 0 {
            let _ = tokio::time::timeout(Duration::from_secs(ROLLUP_DEBOUNCE_SECS), rx.recv()).await;
        }

        // 执行上传（仅在 S3 auto_sync 开启时）
        if let Err(e) = run_auto_upload(&db).await {
            log::warn!("[UsageSync][Auto] upload failed: {e}");
        }
    }
}

async fn run_auto_upload(db: &Database) -> Result<(), crate::error::AppError> {
    let Some(s) = settings::get_s3_sync_settings() else {
        return Ok(());
    };
    if !s.enabled || !s.auto_sync {
        return Ok(());
    }
    usage_sync::upload_usage(db, &s).await
}
