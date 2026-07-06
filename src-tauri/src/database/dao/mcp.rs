//! MCP 服务器数据访问对象
//!
//! 提供 MCP 服务器的 CRUD 操作。

use crate::app_config::{McpApps, McpServer};
use crate::database::{lock_conn, Database};
use crate::error::AppError;
use indexmap::IndexMap;
use rusqlite::params;

impl Database {
    /// 获取所有 MCP 服务器
    pub fn get_all_mcp_servers(&self) -> Result<IndexMap<String, McpServer>, AppError> {
        let conn = lock_conn!(self.conn);
        let mut stmt = conn.prepare(
            "SELECT id, name, server_config, description, homepage, docs, tags
             FROM mcp_servers
             ORDER BY name ASC, id ASC"
        ).map_err(|e| AppError::Database(e.to_string()))?;

        let server_iter = stmt
            .query_map([], |row| {
                let id: String = row.get(0)?;
                let name: String = row.get(1)?;
                let server_config_str: String = row.get(2)?;
                let description: Option<String> = row.get(3)?;
                let homepage: Option<String> = row.get(4)?;
                let docs: Option<String> = row.get(5)?;
                let tags_str: String = row.get(6)?;

                let server = serde_json::from_str(&server_config_str).unwrap_or_default();
                let tags = serde_json::from_str(&tags_str).unwrap_or_default();

                Ok((
                    id,
                    name,
                    server,
                    description,
                    homepage,
                    docs,
                    tags,
                ))
            })
            .map_err(|e| AppError::Database(e.to_string()))?;

        let mut servers = IndexMap::new();
        for server_res in server_iter {
            let (id, name, server, description, homepage, docs, tags) =
                server_res.map_err(|e| AppError::Database(e.to_string()))?;
            // v12: per-app 启用标志从本地 settings 读取
            let flags = crate::settings::get_mcp_apps(&id);
            servers.insert(
                id.clone(),
                McpServer {
                    id,
                    name,
                    server,
                    apps: McpApps {
                        claude: flags.claude,
                        codex: flags.codex,
                        gemini: flags.gemini,
                        opencode: flags.opencode,
                        hermes: flags.hermes,
                    },
                    description,
                    homepage,
                    docs,
                    tags,
                },
            );
        }
        Ok(servers)
    }

    /// 保存 MCP 服务器
    pub fn save_mcp_server(&self, server: &McpServer) -> Result<(), AppError> {
        // v12: 定义写 DB，per-app 启用写本地 settings。
        let conn = lock_conn!(self.conn);
        conn.execute(
            "INSERT OR REPLACE INTO mcp_servers (
                id, name, server_config, description, homepage, docs, tags
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                server.id,
                server.name,
                serde_json::to_string(&server.server).map_err(|e| AppError::Database(format!(
                    "Failed to serialize server config: {e}"
                )))?,
                server.description,
                server.homepage,
                server.docs,
                serde_json::to_string(&server.tags)
                    .map_err(|e| AppError::Database(format!("Failed to serialize tags: {e}")))?,
            ],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;

        // 启用标志写入 settings
        let flags = crate::settings::AppFlags {
            claude: server.apps.claude,
            codex: server.apps.codex,
            gemini: server.apps.gemini,
            opencode: server.apps.opencode,
            hermes: server.apps.hermes,
        };
        crate::settings::set_mcp_apps(&server.id, flags)?;

        Ok(())
    }

    /// 删除 MCP 服务器
    pub fn delete_mcp_server(&self, id: &str) -> Result<(), AppError> {
        let conn = lock_conn!(self.conn);
        conn.execute("DELETE FROM mcp_servers WHERE id = ?1", params![id])
            .map_err(|e| AppError::Database(e.to_string()))?;
        // 同步清理本地 settings 中的激活记录
        let _ = crate::settings::remove_mcp_apps(id);
        Ok(())
    }
}
