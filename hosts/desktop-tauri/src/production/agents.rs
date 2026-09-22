//! 生产组合根的 Agent 注册与默认配置行补齐。

use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::Arc,
};

use agentnotify_agent_antigravity::{ANTIGRAVITY_AGENT_ID, AntigravityAgent};
use agentnotify_agent_codex::{CODEX_AGENT_ID, CodexAgent};
use agentnotify_agent_commandcode::{
    COMMANDCODE_AGENT_ID, CommandCodeAgent, CommandCodeReplyInbox, write_reply_window,
};
use agentnotify_agent_devin::{
    DEVIN_AGENT_ID, DevinAgent, DevinDesktopSessions, DevinReplyInbox, DevinSessions,
};
use agentnotify_agent_opencode::{OpenCodeAgent, OpenCodeReplyInbox};
use agentnotify_agent_sdk::{AgentAdapter, AgentRegistry};
use agentnotify_storage_sqlite::{AgentConfigRecord, SqliteStore};

use crate::bridge::error::CommandError;
use crate::platform::AppPaths;

/// 应用自身的回复收件箱目录名；这些目录属于 AgentNotify 自己的数据，
/// 必须随 `AppPaths.config_dir` 迁移，不能落到用户主目录或外部 Agent 目录。
const OPENCODE_REPLY_INBOX_DIR: &str = "opencode-reply-inbox";
const DEVIN_REPLY_INBOX_DIR: &str = "devin-reply-inbox";
const COMMANDCODE_REPLY_INBOX_DIR: &str = "commandcode-reply-inbox";

/// 各适配器 `config_schema` 里的配置键；用户显式配置时优先于默认安装位置。
const CODEX_HOME_KEY: &str = "codexHome";
const ANTIGRAVITY_ANNOTATIONS_DIR_KEY: &str = "annotationsDir";
const ANTIGRAVITY_HOOKS_PATH_KEY: &str = "hooksPath";
const DEVIN_SESSIONS_DATABASE_KEY: &str = "sessionsDatabase";
const DEVIN_DESKTOP_STATE_DATABASE_KEY: &str = "desktopStateDatabase";
const DEVIN_REPLY_INBOX_KEY: &str = "replyInbox";
const COMMANDCODE_REPLY_WINDOW_SEC_KEY: &str = "commandCodeReplyWindowSec";

/// 没有配置行时按启用处理的 Agent：只有 OpenCode 保持这一历史默认，
/// 免得升级后把现网正在工作的通知链路静默关掉。
const AGENT_IDS_ENABLED_WITHOUT_CONFIG: [&str; 1] = ["opencode"];

/// 读取数据库里保存的 Agent 配置；失败时明确报错，不用默认值猜测。
pub(super) async fn load_agent_configs(
    store: &SqliteStore,
) -> Result<BTreeMap<String, AgentConfigRecord>, CommandError> {
    store
        .agent_configs()
        .await
        .map_err(|error| CommandError::new("agent_configs_query_failed", error.to_string()))
}

/// 注册全部 Agent 适配器，并按数据库里保存的配置覆盖默认安装位置。
///
/// `paths` 只用于应用自身的数据目录（回复收件箱）；Codex、Antigravity、Devin、CommandCode
/// 的外部数据（`%USERPROFILE%\.codex`、`%USERPROFILE%\.gemini`、`%APPDATA%\devin`、
/// `%USERPROFILE%\.commandcode`）是外部工具的真实安装位置，由适配器自己解析。
/// 用户在界面上显式配置的路径以配置为准，缺省或清空时继续用各自的默认位置。
///
/// 注册只构造对象：不启动进程、不连网，也不创建任何目录。
pub(super) fn build_agent_registry(
    paths: &AppPaths,
    configs: &BTreeMap<String, AgentConfigRecord>,
) -> Result<AgentRegistry, CommandError> {
    let adapters: Vec<Arc<dyn AgentAdapter>> = vec![
        Arc::new(OpenCodeAgent::new(OpenCodeReplyInbox::new(
            reply_inbox_root(&paths.config_dir, OPENCODE_REPLY_INBOX_DIR),
        ))),
        Arc::new(build_codex_agent(configs.get(CODEX_AGENT_ID))?),
        Arc::new(build_antigravity_agent(configs.get(ANTIGRAVITY_AGENT_ID))?),
        Arc::new(build_devin_agent(paths, configs.get(DEVIN_AGENT_ID))?),
        Arc::new(build_commandcode_agent(
            paths,
            configs.get(COMMANDCODE_AGENT_ID),
        )?),
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

fn build_codex_agent(record: Option<&AgentConfigRecord>) -> Result<CodexAgent, CommandError> {
    let Some(config) = record.map(|record| &record.config) else {
        return Ok(CodexAgent::from_default_location());
    };
    match configured_path(config, CODEX_HOME_KEY)? {
        Some(codex_home) => Ok(CodexAgent::new(codex_home)),
        None => Ok(CodexAgent::from_default_location()),
    }
}

fn build_antigravity_agent(
    record: Option<&AgentConfigRecord>,
) -> Result<AntigravityAgent, CommandError> {
    let mut agent = AntigravityAgent::from_default_location();
    let Some(config) = record.map(|record| &record.config) else {
        return Ok(agent);
    };
    if let Some(annotations_dir) = configured_path(config, ANTIGRAVITY_ANNOTATIONS_DIR_KEY)? {
        agent = agent.with_annotations_dir(annotations_dir);
    }
    if let Some(hooks_path) = configured_path(config, ANTIGRAVITY_HOOKS_PATH_KEY)? {
        agent = agent.with_hooks_path(hooks_path);
    }
    Ok(agent)
}

fn build_devin_agent(
    paths: &AppPaths,
    record: Option<&AgentConfigRecord>,
) -> Result<DevinAgent, CommandError> {
    // 收件箱默认随 AppPaths 隔离；用户显式配置时才改用配置路径。
    let mut agent = DevinAgent::from_default_location().with_inbox(DevinReplyInbox::new(
        reply_inbox_root(&paths.config_dir, DEVIN_REPLY_INBOX_DIR),
    ));
    let Some(config) = record.map(|record| &record.config) else {
        return Ok(agent);
    };
    if let Some(sessions_database) = configured_path(config, DEVIN_SESSIONS_DATABASE_KEY)? {
        agent = agent.with_sessions(DevinSessions::new(sessions_database));
    }
    if let Some(desktop_database) = configured_path(config, DEVIN_DESKTOP_STATE_DATABASE_KEY)? {
        agent = agent.with_desktop(DevinDesktopSessions::new(desktop_database));
    }
    if let Some(reply_inbox) = configured_path(config, DEVIN_REPLY_INBOX_KEY)? {
        agent = agent.with_inbox(DevinReplyInbox::new(reply_inbox));
    }
    Ok(agent)
}

fn build_commandcode_agent(
    paths: &AppPaths,
    record: Option<&AgentConfigRecord>,
) -> Result<CommandCodeAgent, CommandError> {
    let mut agent =
        CommandCodeAgent::from_default_location().with_inbox(CommandCodeReplyInbox::new(
            reply_inbox_root(&paths.config_dir, COMMANDCODE_REPLY_INBOX_DIR),
        ));
    let Some(config) = record.map(|record| &record.config) else {
        return Ok(agent);
    };
    if let Some(reply_window_sec) = configured_u64(config, COMMANDCODE_REPLY_WINDOW_SEC_KEY)? {
        agent = agent.with_reply_window_sec(reply_window_sec);
    }
    Ok(agent)
}

/// 启动装配：注册全部适配器，并把 Command Code 回复窗口写给 mod。
///
/// `build_agent_registry` 保持“只构造对象、不落盘”；启动时的唯一落盘（窗口文件）
/// 发生在这一层，启动与保存两条路径共用 `sync_commandcode_reply_window`。
pub(super) fn assemble_agents(
    paths: &AppPaths,
    configs: &BTreeMap<String, AgentConfigRecord>,
) -> Result<AgentRegistry, CommandError> {
    let registry = build_agent_registry(paths, configs)?;
    sync_commandcode_reply_window(paths, configs)?;
    Ok(registry)
}

/// 把界面配置的 Command Code 回复窗口写进 mod 能读到的 `window.json`。
///
/// 与适配器同源：复用 `build_commandcode_agent`，直接取装配后生效的值，不另外
/// 解析一遍配置；写入幂等、原子替换。启动装配与 `update_agent_config` 保存后都必须调用。
pub(super) fn sync_commandcode_reply_window(
    paths: &AppPaths,
    configs: &BTreeMap<String, AgentConfigRecord>,
) -> Result<(), CommandError> {
    let agent = build_commandcode_agent(paths, configs.get(COMMANDCODE_AGENT_ID))?;
    let Some(inbox_root) = agent.inbox().root() else {
        return Err(CommandError::new(
            "commandcode_reply_inbox_unavailable",
            "Command Code 回复收件箱目录不可用，无法写入回复窗口配置",
        ));
    };
    write_reply_window(inbox_root, agent.reply_window_sec()).map_err(|error| {
        CommandError::new(
            "commandcode_reply_window_write_failed",
            format!("写入 Command Code 回复窗口文件失败：{error}"),
        )
    })
}

/// 读取配置里的路径字段：缺省、`null` 或空字符串表示使用默认位置；
/// 非字符串与相对路径属于无效配置，明确报错而不是猜测目标。
fn configured_path(config: &serde_json::Value, key: &str) -> Result<Option<PathBuf>, CommandError> {
    let Some(value) = config.get(key) else {
        return Ok(None);
    };
    if value.is_null() {
        return Ok(None);
    }
    let Some(text) = value.as_str() else {
        return Err(invalid_config(key, "必须是字符串路径"));
    };
    let text = text.trim();
    if text.is_empty() {
        return Ok(None);
    }
    let path = PathBuf::from(text);
    if !path.is_absolute() {
        return Err(invalid_config(key, "必须是绝对路径"));
    }
    Ok(Some(path))
}

/// 读取配置里的非负整数字段：缺省或 `null` 表示使用默认值。
fn configured_u64(config: &serde_json::Value, key: &str) -> Result<Option<u64>, CommandError> {
    let Some(value) = config.get(key) else {
        return Ok(None);
    };
    if value.is_null() {
        return Ok(None);
    }
    value
        .as_u64()
        .map(Some)
        .ok_or_else(|| invalid_config(key, "必须是非负整数"))
}

fn invalid_config(key: &str, reason: &str) -> CommandError {
    CommandError::new(
        "agent_config_invalid",
        format!("Agent 配置项 {key} 无效：{reason}"),
    )
}

/// 应用自身的回复收件箱根目录。
fn reply_inbox_root(config_dir: &Path, inbox_dir_name: &str) -> PathBuf {
    config_dir.join(inbox_dir_name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use agentnotify_domain::Timestamp;
    use agentnotify_testkit::test_temp_root;

    fn temp_root(prefix: &str) -> tempfile::TempDir {
        tempfile::Builder::new()
            .prefix(prefix)
            .tempdir_in(test_temp_root())
            .expect("测试临时目录必须可创建")
    }

    fn record(config: serde_json::Value) -> AgentConfigRecord {
        AgentConfigRecord {
            enabled: true,
            config,
            updated_at: Timestamp::now_utc(),
        }
    }

    fn inbox_root(paths: &AppPaths, dir_name: &str) -> PathBuf {
        reply_inbox_root(&paths.config_dir, dir_name)
    }

    fn window_file_path(paths: &AppPaths) -> PathBuf {
        inbox_root(paths, COMMANDCODE_REPLY_INBOX_DIR)
            .join(agentnotify_agent_commandcode::WINDOW_FILE_NAME)
    }

    fn window_file_value(path: &Path) -> serde_json::Value {
        let content = std::fs::read_to_string(path)
            .unwrap_or_else(|error| panic!("窗口文件必须存在 {}：{error}", path.display()));
        serde_json::from_str(&content).expect("窗口文件必须是合法 JSON")
    }

    /// 注册只构造对象：必须注册全部适配器，且不创建任何目录。
    #[test]
    fn building_the_registry_registers_every_agent_without_touching_disk() {
        let root = temp_root("agentnotify-agent-registry-test-");
        let isolated = root.path().join("isolated");
        let paths = AppPaths::for_tests(&isolated);

        let registry =
            build_agent_registry(&paths, &BTreeMap::new()).expect("注册全部 Agent 必须成功");

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
        let isolated = test_temp_root().join("agentnotify-agent-registry-isolated");
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

    /// 启动装配与保存都必须把 Command Code 回复窗口写给 mod，且与适配器同源。
    #[test]
    fn commandcode_reply_window_file_is_written_for_the_mod() {
        let root = temp_root("agentnotify-agent-window-test-");
        let isolated = root.path().join("isolated");
        let paths = AppPaths::for_tests(&isolated);
        let configs = |sec: u64| {
            BTreeMap::from([(
                COMMANDCODE_AGENT_ID.to_string(),
                record(serde_json::json!({ "commandCodeReplyWindowSec": sec })),
            )])
        };

        // 启动装配写一次：内容必须与界面里保存的值一致。
        assemble_agents(&paths, &configs(300)).expect("启动装配必须成功");
        let window_file = window_file_path(&paths);
        assert_eq!(
            window_file_value(&window_file),
            serde_json::json!({ "commandCodeReplyWindowSec": 300 })
        );

        // 保存后同步幂等，并覆盖旧值（保存 0 = 关闭，绝不能留下旧窗口）。
        sync_commandcode_reply_window(&paths, &configs(0)).expect("保存后同步必须成功");
        assert_eq!(
            window_file_value(&window_file),
            serde_json::json!({ "commandCodeReplyWindowSec": 0 })
        );

        // 收件箱属于应用自己的数据：必须落在隔离根下，不得落到用户主目录。
        assert!(
            window_file.starts_with(&isolated),
            "{}",
            window_file.display()
        );
        if let Some(profile) = std::env::var_os("USERPROFILE").map(PathBuf::from) {
            if !isolated.starts_with(&profile) {
                assert!(
                    !window_file.starts_with(&profile),
                    "{}",
                    window_file.display()
                );
            }
        }
    }

    /// 保存的配置必须覆盖各适配器的默认位置。
    #[test]
    fn saved_agent_configs_override_default_locations() {
        let root = temp_root("agentnotify-agent-config-test-");
        let paths = AppPaths::for_tests(root.path());

        let codex_home = root.path().join("codex-home");
        let codex = build_codex_agent(Some(&record(serde_json::json!({
            "codexHome": codex_home,
        }))))
        .expect("Codex 配置必须可解析");
        assert_eq!(codex.codex_home(), Some(codex_home.as_path()));

        let annotations_dir = root.path().join("antigravity-annotations");
        let hooks_path = root.path().join("hooks.json");
        let antigravity = build_antigravity_agent(Some(&record(serde_json::json!({
            "annotationsDir": annotations_dir,
            "hooksPath": hooks_path,
        }))))
        .expect("Antigravity 配置必须可解析");
        assert_eq!(
            antigravity.annotations_dir(),
            Some(annotations_dir.as_path())
        );
        assert_eq!(antigravity.hooks_path(), Some(hooks_path.as_path()));

        let reply_inbox = root.path().join("devin-inbox");
        let devin = build_devin_agent(
            &paths,
            Some(&record(serde_json::json!({
                "sessionsDatabase": root.path().join("sessions.db"),
                "desktopStateDatabase": root.path().join("state.vscdb"),
                "replyInbox": reply_inbox,
            }))),
        )
        .expect("Devin 配置必须可解析");
        assert_eq!(devin.inbox().root(), Some(reply_inbox.as_path()));

        let commandcode = build_commandcode_agent(
            &paths,
            Some(&record(serde_json::json!({
                "commandCodeReplyWindowSec": 120,
            }))),
        )
        .expect("CommandCode 配置必须可解析");
        assert_eq!(commandcode.reply_window_sec(), 120);
        let default_commandcode_inbox = inbox_root(&paths, COMMANDCODE_REPLY_INBOX_DIR);
        assert_eq!(
            commandcode.inbox().root(),
            Some(default_commandcode_inbox.as_path())
        );
    }

    /// 缺省、空字符串与 `null` 都必须保持各适配器的默认行为。
    #[test]
    fn missing_or_cleared_config_keeps_adapter_defaults() {
        let root = temp_root("agentnotify-agent-config-default-test-");
        let paths = AppPaths::for_tests(root.path());

        let codex = build_codex_agent(None).expect("缺省配置必须可构造");
        assert_eq!(
            codex.codex_home(),
            agentnotify_agent_codex::default_codex_home().as_deref()
        );
        let cleared_codex = build_codex_agent(Some(&record(serde_json::json!({
            "codexHome": "  ",
        }))))
        .expect("清空的配置必须按缺省处理");
        assert_eq!(
            cleared_codex.codex_home(),
            agentnotify_agent_codex::default_codex_home().as_deref()
        );

        let antigravity = build_antigravity_agent(None).expect("缺省配置必须可构造");
        assert_eq!(
            antigravity.annotations_dir(),
            agentnotify_agent_antigravity::default_annotations_dir().as_deref()
        );

        let devin = build_devin_agent(&paths, None).expect("缺省配置必须可构造");
        let devin_inbox = inbox_root(&paths, DEVIN_REPLY_INBOX_DIR);
        assert_eq!(devin.inbox().root(), Some(devin_inbox.as_path()));

        let commandcode = build_commandcode_agent(&paths, None).expect("缺省配置必须可构造");
        assert_eq!(
            commandcode.reply_window_sec(),
            agentnotify_agent_commandcode::resolve_reply_window_sec()
        );
        let commandcode_inbox = inbox_root(&paths, COMMANDCODE_REPLY_INBOX_DIR);
        assert_eq!(
            commandcode.inbox().root(),
            Some(commandcode_inbox.as_path())
        );
    }

    /// 无效配置必须明确报错，不能悄悄回退到默认位置。
    #[test]
    fn invalid_agent_config_is_rejected_with_a_clear_error() {
        for config in [
            serde_json::json!({"codexHome": 42}),
            serde_json::json!({"codexHome": "relative\\codex"}),
        ] {
            let error = match build_codex_agent(Some(&record(config))) {
                Ok(_) => panic!("无效配置必须报错"),
                Err(error) => error,
            };
            assert_eq!(error.code(), "agent_config_invalid");
            assert!(error.message().contains("codexHome"), "{}", error.message());
        }

        let invalid_paths =
            AppPaths::for_tests(test_temp_root().join("agentnotify-agent-config-invalid"));
        let error = match build_commandcode_agent(
            &invalid_paths,
            Some(&record(serde_json::json!({
                "commandCodeReplyWindowSec": -1,
            }))),
        ) {
            Ok(_) => panic!("负数窗口必须报错"),
            Err(error) => error,
        };
        assert_eq!(error.code(), "agent_config_invalid");
        assert!(
            error.message().contains("commandCodeReplyWindowSec"),
            "{}",
            error.message()
        );
    }
}
