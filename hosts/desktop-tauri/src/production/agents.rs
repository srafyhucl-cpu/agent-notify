//! 生产组合根的 Agent 注册与默认配置行补齐。

use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use agentnotify_agent_antigravity::AntigravityAgent;
use agentnotify_agent_codex::CodexAgent;
use agentnotify_agent_commandcode::{CommandCodeAgent, CommandCodeReplyInbox};
use agentnotify_agent_devin::{DevinAgent, DevinReplyInbox};
use agentnotify_agent_opencode::{OpenCodeAgent, OpenCodeReplyInbox};
use agentnotify_agent_sdk::{AgentAdapter, AgentRegistry};
use agentnotify_storage_sqlite::SqliteStore;

use crate::bridge::error::CommandError;
use crate::platform::AppPaths;

/// 应用自身的回复收件箱目录名；这些目录属于 AgentNotify 自己的数据，
/// 必须随 `AppPaths.config_dir` 迁移，不能落到用户主目录或外部 Agent 目录。
const OPENCODE_REPLY_INBOX_DIR: &str = "opencode-reply-inbox";
const DEVIN_REPLY_INBOX_DIR: &str = "devin-reply-inbox";
const COMMANDCODE_REPLY_INBOX_DIR: &str = "commandcode-reply-inbox";

/// 没有配置行时按启用处理的 Agent：只有 OpenCode 保持这一历史默认，
/// 免得升级后把现网正在工作的通知链路静默关掉。
const AGENT_IDS_ENABLED_WITHOUT_CONFIG: [&str; 1] = ["opencode"];

/// 注册全部 Agent 适配器。
///
/// `paths` 只用于应用自身的数据目录（回复收件箱）；Codex、Antigravity、Devin、CommandCode
/// 的外部数据（`%USERPROFILE%\.codex`、`%USERPROFILE%\.gemini`、`%APPDATA%\devin`、
/// `%USERPROFILE%\.commandcode`）是外部工具的真实安装位置，由适配器自己解析。
///
/// 注册只构造对象：不启动进程、不连网，也不创建任何目录。
pub(super) fn build_agent_registry(paths: &AppPaths) -> Result<AgentRegistry, CommandError> {
    let adapters: Vec<Arc<dyn AgentAdapter>> = vec![
        Arc::new(OpenCodeAgent::new(OpenCodeReplyInbox::new(
            reply_inbox_root(&paths.config_dir, OPENCODE_REPLY_INBOX_DIR),
        ))),
        Arc::new(CodexAgent::from_default_location()),
        Arc::new(AntigravityAgent::from_default_location()),
        Arc::new(
            DevinAgent::from_default_location().with_inbox(DevinReplyInbox::new(reply_inbox_root(
                &paths.config_dir,
                DEVIN_REPLY_INBOX_DIR,
            ))),
        ),
        Arc::new(
            CommandCodeAgent::from_default_location().with_inbox(CommandCodeReplyInbox::new(
                reply_inbox_root(&paths.config_dir, COMMANDCODE_REPLY_INBOX_DIR),
            )),
        ),
    ];

    let mut registry = AgentRegistry::default();
    for adapter in adapters {
        registry
            .register(adapter)
            .map_err(|error| CommandError::new("agent_register_failed", error.to_string()))?;
    }
    Ok(registry)
}

/// 补齐默认关闭的配置行：运行时对没有配置行的 Agent 按启用处理，
/// 新接入的适配器必须先写入 `enabled = false`，由用户在界面显式启用。
///
/// 只插入缺失行：已有配置（例如迁移按 `devin.off` 写入的关闭行、用户在界面上改过的配置）
/// 原样保留，绝不覆盖。
pub(super) async fn seed_disabled_agent_configs(
    store: &SqliteStore,
    registry: &AgentRegistry,
) -> Result<(), CommandError> {
    let existing = store
        .agent_configs()
        .await
        .map_err(|error| CommandError::new("agent_configs_query_failed", error.to_string()))?;

    for adapter in registry.all() {
        let agent_id = adapter.descriptor().id;
        if AGENT_IDS_ENABLED_WITHOUT_CONFIG.contains(&agent_id.as_str())
            || existing.contains_key(agent_id.as_str())
        {
            continue;
        }
        // 空对象与旧数据迁移、update_agent_config 的默认配置形状一致。
        store
            .upsert_agent_config(agent_id.as_str(), false, &serde_json::json!({}))
            .await
            .map_err(|error| CommandError::new("agent_config_seed_failed", error.to_string()))?;
    }
    Ok(())
}

/// 应用自身的回复收件箱根目录。
fn reply_inbox_root(config_dir: &Path, inbox_dir_name: &str) -> PathBuf {
    config_dir.join(inbox_dir_name)
}

#[cfg(test)]
mod tests {
    use super::*;

    const D_DRIVE_TEMP: &str = r"D:\Temp";

    /// 注册只构造对象：必须注册全部适配器，且不创建任何目录。
    #[test]
    fn building_the_registry_registers_every_agent_without_touching_disk() {
        let root = tempfile::Builder::new()
            .prefix("agentnotify-agent-registry-test-")
            .tempdir_in(D_DRIVE_TEMP)
            .expect("D 盘测试目录必须可创建");
        let isolated = root.path().join("isolated");
        let paths = AppPaths::for_tests(&isolated);

        let registry = build_agent_registry(&paths).expect("注册全部 Agent 必须成功");

        let ids: Vec<String> = registry
            .all()
            .iter()
            .map(|adapter| adapter.descriptor().id.to_string())
            .collect();
        assert_eq!(
            ids,
            ["antigravity", "codex", "commandcode", "devin", "opencode"]
        );
        assert!(!isolated.exists(), "注册阶段不得创建任何目录");
    }

    /// 收件箱根目录由 AppPaths 派生：隔离根下不会出现外部工具目录，也不会落到用户主目录。
    #[test]
    fn reply_inbox_roots_follow_the_configured_app_paths() {
        let isolated = PathBuf::from(r"D:\Temp\agentnotify-agent-registry-isolated");
        let paths = AppPaths::for_tests(&isolated);

        for inbox_dir_name in [
            OPENCODE_REPLY_INBOX_DIR,
            DEVIN_REPLY_INBOX_DIR,
            COMMANDCODE_REPLY_INBOX_DIR,
        ] {
            let root = reply_inbox_root(&paths.config_dir, inbox_dir_name);
            assert_eq!(root.parent(), Some(paths.config_dir.as_path()));
            assert!(root.starts_with(&isolated), "{}", root.display());
            if let Some(profile) = std::env::var_os("USERPROFILE").map(PathBuf::from) {
                // 隔离根本身不在用户主目录下时，收件箱也不允许落在用户主目录里。
                if !isolated.starts_with(&profile) {
                    assert!(!root.starts_with(&profile), "{}", root.display());
                }
            }
        }
    }
}
