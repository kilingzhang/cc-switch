//! 故障转移队列 DAO
//!
//! v12 起，故障转移队列状态（原 providers.in_failover_queue 列）迁移至
//! 本地 settings.json 的 device_activation.failover_queue，确保配置同步
//! 只传 provider 定义、不传各设备的故障转移选择。

use crate::database::Database;
use crate::error::AppError;
use crate::provider::Provider;
use serde::{Deserialize, Serialize};

/// 故障转移队列条目（简化版，用于前端展示）
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FailoverQueueItem {
    pub provider_id: String,
    pub provider_name: String,
    pub sort_index: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider_notes: Option<String>,
}

impl Database {
    /// 获取故障转移队列（按 sort_index 排序）
    pub fn get_failover_queue(&self, app_type: &str) -> Result<Vec<FailoverQueueItem>, AppError> {
        let queue_ids = crate::settings::get_failover_queue(app_type);
        if queue_ids.is_empty() {
            return Ok(Vec::new());
        }

        let providers = self.get_all_providers(app_type)?;
        // 保持 settings 中的顺序（即加入顺序）
        let items = queue_ids
            .iter()
            .filter_map(|id| providers.get(id))
            .map(|p| FailoverQueueItem {
                provider_id: p.id.clone(),
                provider_name: p.name.clone(),
                sort_index: p.sort_index,
                provider_notes: p.notes.clone(),
            })
            .collect();
        Ok(items)
    }

    /// 获取故障转移队列中的供应商（完整 Provider 信息，按顺序）
    pub fn get_failover_providers(&self, app_type: &str) -> Result<Vec<Provider>, AppError> {
        let all_providers = self.get_all_providers(app_type)?;
        let queue_ids = crate::settings::get_failover_queue(app_type);

        // 按 settings 中的顺序输出
        let result: Vec<Provider> = queue_ids
            .iter()
            .filter_map(|id| all_providers.get(id).cloned())
            .collect();
        Ok(result)
    }

    /// 添加供应商到故障转移队列
    pub fn add_to_failover_queue(&self, app_type: &str, provider_id: &str) -> Result<(), AppError> {
        crate::settings::add_to_failover_queue(app_type, provider_id)?;
        Ok(())
    }

    /// 从故障转移队列中移除供应商
    pub fn remove_from_failover_queue(
        &self,
        app_type: &str,
        provider_id: &str,
    ) -> Result<(), AppError> {
        // 1. 从 settings 移除
        crate::settings::remove_from_failover_queue(app_type, provider_id)?;

        // 2. 清除该供应商的健康状态（退出队列后不再需要健康监控）
        let conn = crate::database::lock_conn!(self.conn);
        conn.execute(
            "DELETE FROM provider_health WHERE provider_id = ?1 AND app_type = ?2",
            rusqlite::params![provider_id, app_type],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;

        log::info!("已从故障转移队列移除供应商 {provider_id} ({app_type}), 并清除其健康状态");

        Ok(())
    }

    /// 清空故障转移队列
    pub fn clear_failover_queue(&self, app_type: &str) -> Result<(), AppError> {
        crate::settings::clear_failover_queue(app_type)?;
        Ok(())
    }

    /// 检查供应商是否在故障转移队列中
    pub fn is_in_failover_queue(
        &self,
        app_type: &str,
        provider_id: &str,
    ) -> Result<bool, AppError> {
        Ok(crate::settings::is_in_failover_queue(app_type, provider_id))
    }

    /// 获取可添加到故障转移队列的供应商（不在队列中的）
    pub fn get_available_providers_for_failover(
        &self,
        app_type: &str,
    ) -> Result<Vec<Provider>, AppError> {
        let all_providers = self.get_all_providers(app_type)?;
        let available: Vec<Provider> = all_providers
            .into_values()
            .filter(|p| !p.in_failover_queue)
            .collect();
        Ok(available)
    }
}
