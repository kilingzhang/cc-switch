//! Skills 数据访问对象
//!
//! 提供 Skills 和 Skill Repos 的 CRUD 操作。
//!
//! v3.10.0+ 统一管理架构：
//! - Skills 使用统一的 id 主键，支持四应用启用标志
//! - 实际文件存储在 ~/.cc-switch/skills/，同步到各应用目录

use crate::app_config::{InstalledSkill, SkillApps};
use crate::database::{lock_conn, Database};
use crate::error::AppError;
use crate::services::skill::SkillRepo;
use indexmap::IndexMap;
use rusqlite::params;

impl Database {
    // ========== InstalledSkill CRUD ==========

    /// 获取所有已安装的 Skills
    pub fn get_all_installed_skills(&self) -> Result<IndexMap<String, InstalledSkill>, AppError> {
        let conn = lock_conn!(self.conn);
        let mut stmt = conn
            .prepare(
                "SELECT id, name, description, directory, repo_owner, repo_name, repo_branch,
                        readme_url, installed_at, content_hash, updated_at
                 FROM skills ORDER BY name ASC",
            )
            .map_err(|e| AppError::Database(e.to_string()))?;

        let skill_iter = stmt
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,       // id
                    row.get::<_, String>(1)?,       // name
                    row.get::<_, Option<String>>(2)?, // description
                    row.get::<_, String>(3)?,       // directory
                    row.get::<_, Option<String>>(4)?, // repo_owner
                    row.get::<_, Option<String>>(5)?, // repo_name
                    row.get::<_, Option<String>>(6)?, // repo_branch
                    row.get::<_, Option<String>>(7)?, // readme_url
                    row.get::<_, i64>(8)?,           // installed_at
                    row.get::<_, Option<String>>(9)?, // content_hash
                    row.get::<_, i64>(10).unwrap_or(0), // updated_at
                ))
            })
            .map_err(|e| AppError::Database(e.to_string()))?;

        let mut skills = IndexMap::new();
        for skill_res in skill_iter {
            let (
                id,
                name,
                description,
                directory,
                repo_owner,
                repo_name,
                repo_branch,
                readme_url,
                installed_at,
                content_hash,
                updated_at,
            ) = skill_res.map_err(|e| AppError::Database(e.to_string()))?;
            // v12: per-app 启用标志从本地 settings 读取
            let flags = crate::settings::get_skill_apps(&id);
            skills.insert(
                id.clone(),
                InstalledSkill {
                    id,
                    name,
                    description,
                    directory,
                    repo_owner,
                    repo_name,
                    repo_branch,
                    readme_url,
                    apps: SkillApps {
                        claude: flags.claude,
                        codex: flags.codex,
                        gemini: flags.gemini,
                        opencode: flags.opencode,
                        hermes: flags.hermes,
                    },
                    installed_at,
                    content_hash,
                    updated_at,
                },
            );
        }
        Ok(skills)
    }

    /// 获取单个已安装的 Skill
    pub fn get_installed_skill(&self, id: &str) -> Result<Option<InstalledSkill>, AppError> {
        let conn = lock_conn!(self.conn);
        let result = conn.query_row(
            "SELECT id, name, description, directory, repo_owner, repo_name, repo_branch,
                        readme_url, installed_at, content_hash, updated_at
                 FROM skills WHERE id = ?1",
            [id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Option<String>>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, Option<String>>(4)?,
                    row.get::<_, Option<String>>(5)?,
                    row.get::<_, Option<String>>(6)?,
                    row.get::<_, Option<String>>(7)?,
                    row.get::<_, i64>(8)?,
                    row.get::<_, Option<String>>(9)?,
                    row.get::<_, i64>(10).unwrap_or(0),
                ))
            },
        );

        match result {
            Ok((
                sid,
                name,
                description,
                directory,
                repo_owner,
                repo_name,
                repo_branch,
                readme_url,
                installed_at,
                content_hash,
                updated_at,
            )) => {
                // v12: per-app 启用标志从本地 settings 读取
                let flags = crate::settings::get_skill_apps(&sid);
                Ok(Some(InstalledSkill {
                    id: sid,
                    name,
                    description,
                    directory,
                    repo_owner,
                    repo_name,
                    repo_branch,
                    readme_url,
                    apps: SkillApps {
                        claude: flags.claude,
                        codex: flags.codex,
                        gemini: flags.gemini,
                        opencode: flags.opencode,
                        hermes: flags.hermes,
                    },
                    installed_at,
                    content_hash,
                    updated_at,
                }))
            }
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(AppError::Database(e.to_string())),
        }
    }

    /// 保存 Skill（添加或更新）
    pub fn save_skill(&self, skill: &InstalledSkill) -> Result<(), AppError> {
        // v12: 定义写 DB，per-app 启用写本地 settings。
        let conn = lock_conn!(self.conn);
        conn.execute(
            "INSERT OR REPLACE INTO skills
             (id, name, description, directory, repo_owner, repo_name, repo_branch,
              readme_url, installed_at, content_hash, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            params![
                skill.id,
                skill.name,
                skill.description,
                skill.directory,
                skill.repo_owner,
                skill.repo_name,
                skill.repo_branch,
                skill.readme_url,
                skill.installed_at,
                skill.content_hash,
                skill.updated_at,
            ],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;

        // 启用标志写入 settings
        let flags = crate::settings::AppFlags {
            claude: skill.apps.claude,
            codex: skill.apps.codex,
            gemini: skill.apps.gemini,
            opencode: skill.apps.opencode,
            hermes: skill.apps.hermes,
        };
        crate::settings::set_skill_apps(&skill.id, flags)?;

        Ok(())
    }

    /// 删除 Skill
    pub fn delete_skill(&self, id: &str) -> Result<bool, AppError> {
        let conn = lock_conn!(self.conn);
        let affected = conn
            .execute("DELETE FROM skills WHERE id = ?1", params![id])
            .map_err(|e| AppError::Database(e.to_string()))?;
        // 同步清理本地 settings 中的激活记录
        if affected > 0 {
            let _ = crate::settings::remove_skill_apps(id);
        }
        Ok(affected > 0)
    }

    /// 清空所有 Skills（用于迁移）
    pub fn clear_skills(&self) -> Result<(), AppError> {
        let conn = lock_conn!(self.conn);
        conn.execute("DELETE FROM skills", [])
            .map_err(|e| AppError::Database(e.to_string()))?;
        Ok(())
    }

    /// 更新 Skill 的应用启用状态
    pub fn update_skill_apps(&self, id: &str, apps: &SkillApps) -> Result<bool, AppError> {
        // v12: 启用状态存于本地 settings。
        let exists = self.get_installed_skill(id)?.is_some();
        if exists {
            let flags = crate::settings::AppFlags {
                claude: apps.claude,
                codex: apps.codex,
                gemini: apps.gemini,
                opencode: apps.opencode,
                hermes: apps.hermes,
            };
            crate::settings::set_skill_apps(id, flags)?;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    /// 更新 Skill 的内容哈希和更新时间
    pub fn update_skill_hash(
        &self,
        id: &str,
        content_hash: &str,
        updated_at: i64,
    ) -> Result<bool, AppError> {
        let conn = lock_conn!(self.conn);
        let affected = conn
            .execute(
                "UPDATE skills SET content_hash = ?1, updated_at = ?2 WHERE id = ?3",
                params![content_hash, updated_at, id],
            )
            .map_err(|e| AppError::Database(e.to_string()))?;
        Ok(affected > 0)
    }

    // ========== SkillRepo CRUD（保持原有） ==========

    /// 获取所有 Skill 仓库
    pub fn get_skill_repos(&self) -> Result<Vec<SkillRepo>, AppError> {
        let conn = lock_conn!(self.conn);
        let mut stmt = conn
            .prepare(
                "SELECT owner, name, branch, enabled FROM skill_repos ORDER BY owner ASC, name ASC",
            )
            .map_err(|e| AppError::Database(e.to_string()))?;

        let repo_iter = stmt
            .query_map([], |row| {
                Ok(SkillRepo {
                    owner: row.get(0)?,
                    name: row.get(1)?,
                    branch: row.get(2)?,
                    enabled: row.get(3)?,
                })
            })
            .map_err(|e| AppError::Database(e.to_string()))?;

        let mut repos = Vec::new();
        for repo_res in repo_iter {
            repos.push(repo_res.map_err(|e| AppError::Database(e.to_string()))?);
        }
        Ok(repos)
    }

    /// 保存 Skill 仓库
    pub fn save_skill_repo(&self, repo: &SkillRepo) -> Result<(), AppError> {
        let conn = lock_conn!(self.conn);
        conn.execute(
            "INSERT OR REPLACE INTO skill_repos (owner, name, branch, enabled) VALUES (?1, ?2, ?3, ?4)",
            params![repo.owner, repo.name, repo.branch, repo.enabled],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;
        Ok(())
    }

    /// 删除 Skill 仓库
    pub fn delete_skill_repo(&self, owner: &str, name: &str) -> Result<(), AppError> {
        let conn = lock_conn!(self.conn);
        conn.execute(
            "DELETE FROM skill_repos WHERE owner = ?1 AND name = ?2",
            params![owner, name],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;
        Ok(())
    }

    /// 初始化默认的 Skill 仓库（启动时调用，补充缺失的默认仓库）
    pub fn init_default_skill_repos(&self) -> Result<usize, AppError> {
        // 获取已有仓库列表
        let existing = self.get_skill_repos()?;
        let existing_keys: std::collections::HashSet<(String, String)> = existing
            .iter()
            .map(|r| (r.owner.clone(), r.name.clone()))
            .collect();

        // 获取默认仓库列表
        let default_store = crate::services::skill::SkillStore::default();
        let mut count = 0;

        // 仅插入缺失的默认仓库
        for repo in &default_store.repos {
            let key = (repo.owner.clone(), repo.name.clone());
            if !existing_keys.contains(&key) {
                self.save_skill_repo(repo)?;
                count += 1;
                log::info!("补充默认 Skill 仓库: {}/{}", repo.owner, repo.name);
            }
        }

        if count > 0 {
            log::info!("补充默认 Skill 仓库完成，新增 {count} 个");
        }
        Ok(count)
    }
}
