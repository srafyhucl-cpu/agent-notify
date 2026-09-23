use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};

use agentnotify_application::{
    ChannelAccountStore, DeliveryStore, IngestStore, OutboxItem, SecretError, SecretKind,
    SecretStore, SecretValue,
};
use agentnotify_channel_clawbot::CLAWBOT_CHANNEL_ID;
use agentnotify_desktop::bridge::commands::HostCommandService;
use agentnotify_desktop::bridge::dto::*;
use agentnotify_desktop::platform::AppPaths;
use agentnotify_desktop::platform::windows::{CredentialBackend, WindowsSecretStore};
use agentnotify_desktop::production::{
    ProductionRuntimeCoordinator, ProductionSettingsStore, ProductionTargetProvider,
    bootstrap_headless, bootstrap_headless_with_update_transport,
};
use agentnotify_desktop::update::{HttpTextResponse, UpdateError, UpdateTransport};
use agentnotify_domain::{
    AgentId, AgentSessionId, ChannelAccountId, ChannelId, Delivery, DeliveryId, Notification,
    NotificationId, NotificationMetadata, SafeError, Timestamp,
};
use agentnotify_runtime::{MigrationState, RuntimeState, RuntimeTargetProvider};
use agentnotify_storage_sqlite::SqliteStore;

/// 生产组合根必须注册的全部 Agent；`AgentRegistry::all()` 按 Agent ID 排序。
const EXPECTED_AGENT_IDS: [&str; 5] = ["antigravity", "codex", "commandcode", "devin", "opencode"];
/// 新接入的四个适配器；它们必须先默认关闭，由用户在界面启用。
const NEW_AGENT_IDS: [&str; 4] = ["antigravity", "codex", "commandcode", "devin"];

fn agent_ids(agents: &[AgentDto]) -> Vec<&str> {
    agents.iter().map(|agent| agent.id.as_str()).collect()
}

fn find_agent<'a>(agents: &'a [AgentDto], agent_id: &str) -> &'a AgentDto {
    agents
        .iter()
        .find(|agent| agent.id == agent_id)
        .unwrap_or_else(|| panic!("注册表必须包含 Agent {agent_id}"))
}

#[derive(Default)]
struct MemoryCredentialBackend {
    values: Mutex<BTreeMap<String, String>>,
}

impl CredentialBackend for MemoryCredentialBackend {
    fn read(&self, target: &str) -> Result<Option<SecretValue>, SecretError> {
        self.values
            .lock()
            .expect("锁有效")
            .get(target)
            .cloned()
            .map(SecretValue::new)
            .transpose()
    }

    fn write(&self, target: &str, value: &SecretValue) -> Result<(), SecretError> {
        self.values
            .lock()
            .expect("锁有效")
            .insert(target.to_owned(), value.expose().to_owned());
        Ok(())
    }

    fn delete(&self, target: &str) -> Result<(), SecretError> {
        self.values.lock().expect("锁有效").remove(target);
        Ok(())
    }
}

fn create_test_env(
    prefix: &str,
) -> (
    tempfile::TempDir,
    AppPaths,
    Arc<SqliteStore>,
    Arc<dyn SecretStore>,
) {
    let root = tempfile::Builder::new()
        .prefix(prefix)
        .tempdir_in(agentnotify_testkit::test_temp_root())
        .expect("测试临时目录必须可创建");
    let paths = AppPaths::for_tests(root.path());
    paths.ensure().expect("隔离路径必须可创建");

    let db_path = paths.data_dir.join("state.db");
    let store = Arc::new(SqliteStore::open(&db_path).expect("数据库必须可创建"));

    let backend = Arc::new(MemoryCredentialBackend::default());
    let secret_store: Arc<dyn SecretStore> = Arc::new(WindowsSecretStore::with_backend(backend));

    (root, paths, store, secret_store)
}

#[tokio::test]
async fn settings_store_round_trip_and_legacy_compatibility() {
    let (_root, paths, store, _secret_store) = create_test_env("agentnotify-settings-test-");

    // 写入模拟旧版 settings.json
    let legacy_settings_path = paths.config_dir.join("settings.json");
    tokio::fs::write(
        &legacy_settings_path,
        serde_json::json!({
            "paused": true,
            "cooldown_seconds": 45,
            "reply_enabled": false
        })
        .to_string(),
    )
    .await
    .expect("写入旧版设置必须成功");

    let settings_store = ProductionSettingsStore::new(store.clone(), &paths.config_dir);

    // 首次加载应平滑迁移旧版 pause 状态
    let loaded = settings_store
        .load_settings()
        .await
        .expect("首次加载设置必须成功");
    assert!(loaded.notifications_paused);

    // 保存更新
    let mut updated = loaded.clone();
    updated.cooldown_seconds = 120;
    updated.notifications_paused = false;
    settings_store
        .save_settings(&updated)
        .await
        .expect("保存设置必须成功");

    let reloaded = settings_store
        .load_settings()
        .await
        .expect("重新加载设置必须成功");
    assert_eq!(reloaded.cooldown_seconds, 120);
    assert!(!reloaded.notifications_paused);
}

#[tokio::test]
async fn first_channel_account_becomes_default_without_overwriting_explicit_choice() {
    let (_root, paths, store, _secret_store) = create_test_env("agentnotify-default-account-test-");
    let settings_store = ProductionSettingsStore::new(store, &paths.config_dir);

    assert!(
        settings_store
            .ensure_default_channel_account("clawbot-new")
            .await
            .expect("首次设置默认账号必须成功")
    );

    let settings = settings_store
        .load_settings()
        .await
        .expect("读取设置必须成功");
    assert_eq!(
        settings.default_channel_account_id.as_deref(),
        Some("clawbot-new")
    );

    assert!(
        !settings_store
            .ensure_default_channel_account("clawbot-other")
            .await
            .expect("已有默认账号时必须幂等返回")
    );
    let settings = settings_store
        .load_settings()
        .await
        .expect("重新读取设置必须成功");
    assert_eq!(
        settings.default_channel_account_id.as_deref(),
        Some("clawbot-new")
    );
}

#[tokio::test]
async fn target_provider_resolves_opencode_and_enabled_clawbot_accounts() {
    let (_root, paths, store, secret_store) = create_test_env("agentnotify-targets-test-");
    let settings_store = ProductionSettingsStore::new(store.clone(), &paths.config_dir);

    // 准备 AgentRegistry 与 ChannelRegistry
    let mut agent_registry = agentnotify_agent_sdk::AgentRegistry::default();
    let opencode_inbox = agentnotify_agent_opencode::OpenCodeReplyInbox::new(
        paths.config_dir.join("opencode-reply-inbox"),
    );
    agent_registry
        .register(Arc::new(agentnotify_agent_opencode::OpenCodeAgent::new(
            opencode_inbox,
        )))
        .expect("注册 agent 必须成功");

    let clawbot_channel = agentnotify_channel_clawbot::ClawBotChannel::new(secret_store.clone())
        .with_account_store(store.clone());
    let mut channel_registry = agentnotify_channel_sdk::ChannelRegistry::default();
    channel_registry
        .register(Arc::new(clawbot_channel))
        .expect("注册 channel 必须成功");

    let target_provider = ProductionTargetProvider::new(
        store.clone(),
        settings_store,
        secret_store.clone(),
        Arc::new(agent_registry),
        Arc::new(channel_registry),
    );

    // 初始状态：无渠道账号
    let initial_targets = target_provider
        .resolve()
        .await
        .expect("解析初始目标必须成功");
    assert_eq!(initial_targets.delivery_targets.len(), 0);

    // 插入两个 ClawBot 账号：一个启用，一个禁用
    let enabled_clawbot =
        agentnotify_channel_clawbot::ClawBotAccount::from_platform_ids("test-bot-1", "wx-user-123")
            .expect("构造 ClawBot 启用账号成功");
    let mut enabled_acc = enabled_clawbot
        .into_channel_account()
        .expect("转为 ChannelAccount 成功");
    enabled_acc.enabled = true;
    let enabled_account_id = enabled_acc.id.clone();

    let disabled_clawbot =
        agentnotify_channel_clawbot::ClawBotAccount::from_platform_ids("test-bot-2", "wx-user-456")
            .expect("构造 ClawBot 禁用账号成功");
    let mut disabled_acc = disabled_clawbot
        .into_channel_account()
        .expect("转为 ChannelAccount 成功");
    disabled_acc.enabled = false;
    let _disabled_account_id = disabled_acc.id.clone();

    store
        .upsert(enabled_acc)
        .await
        .expect("upsert 启用账号成功");
    store
        .upsert(disabled_acc)
        .await
        .expect("upsert 禁用账号成功");

    // 写入合法 ClawBot 凭据
    let creds = serde_json::json!({
        "bot_token": "test-bot-token-123",
        "bot_id": "test-bot-1",
        "user_id": "wx-user-123",
        "base_url": "https://ilinkai.weixin.qq.com"
    });
    secret_store
        .set(
            &enabled_account_id,
            SecretKind::BotToken,
            SecretValue::new(creds.to_string()).unwrap(),
        )
        .await
        .expect("写入密钥成功");

    // 重新解析目标：只有启用的账号才会被解析
    let resolved = target_provider.resolve().await.expect("解析目标必须成功");
    assert_eq!(resolved.delivery_targets.len(), 1);
    assert_eq!(
        resolved.delivery_targets[0].account.id.as_str(),
        enabled_account_id.as_str()
    );
    assert_eq!(resolved.delivery_targets[0].conversation_id, "wx-user-123");
}

#[tokio::test]
async fn host_commands_contract_execution() {
    let (_root, paths, _store, secret_store) = create_test_env("agentnotify-host-test-");

    // 更新状态走假传输：契约测试不允许访问真实网络。
    let (coordinator, service) =
        bootstrap_headless_with_update_transport(paths, secret_store, Arc::new(UpToDateTransport))
            .await
            .expect("Headless 装配与启动必须成功");

    // 1. get_snapshot (支持启动中或就绪)
    let mut snapshot = service
        .get_snapshot(EmptyPayload {})
        .await
        .expect("获取 snapshot 必须成功");

    // 轮询等待后台 status.refresher 刷新至 Running 状态（最多 2 秒）
    for _ in 0..20 {
        if snapshot.runtime.state == RuntimeLifecycleStateDto::Running {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        snapshot = service
            .get_snapshot(EmptyPayload {})
            .await
            .expect("轮询获取 snapshot 成功");
    }

    assert_eq!(snapshot.runtime.state, RuntimeLifecycleStateDto::Running);
    assert!(!snapshot.runtime.paused);
    assert_eq!(agent_ids(&snapshot.overview.agents), EXPECTED_AGENT_IDS);

    // 2. list_agents
    let agents = service
        .list_agents(EmptyPayload {})
        .await
        .expect("列出 agents 必须成功");
    assert_eq!(agent_ids(&agents), EXPECTED_AGENT_IDS);
    assert!(
        agents.iter().find(|a| a.id == "opencode").unwrap().enabled,
        "OpenCode 保持旧默认启用"
    );

    // 3. update_agent_config
    let updated_agent = service
        .update_agent_config(UpdateAgentConfigPayload {
            agent_id: "opencode".into(),
            enabled: Some(true),
            config: Some(serde_json::json!({ "customOption": "test-value" })),
        })
        .await
        .expect("更新 agent 配置必须成功");
    assert_eq!(
        updated_agent.config,
        serde_json::json!({ "customOption": "test-value" })
    );

    // 4. list_channel_accounts (此时无账号)
    let channel_list = service
        .list_channel_accounts(EmptyPayload {})
        .await
        .expect("列出 channels 必须成功");
    assert_eq!(channel_list.channels.len(), 1);
    assert_eq!(channel_list.channels[0].id, CLAWBOT_CHANNEL_ID);
    assert_eq!(channel_list.channels[0].accounts.len(), 0);

    // 5. get_settings & update_settings
    let settings = service
        .get_settings(EmptyPayload {})
        .await
        .expect("获取设置必须成功");
    assert!(!settings.notifications_paused);

    let mut new_settings = settings.clone();
    new_settings.cooldown_seconds = 30;
    let updated_settings = service
        .update_settings(new_settings)
        .await
        .expect("更新设置必须成功");
    assert_eq!(updated_settings.cooldown_seconds, 30);

    // 6. set_runtime_paused
    let summary = service
        .set_runtime_paused(SetRuntimePausedPayload { paused: true })
        .await
        .expect("暂停运行时必须成功");
    assert_eq!(summary.state, RuntimeLifecycleStateDto::Paused);
    assert!(summary.paused);

    let current_paused = coordinator.is_outbox_paused().await;
    assert!(current_paused);

    // 恢复
    let summary_restored = service
        .set_runtime_paused(SetRuntimePausedPayload { paused: false })
        .await
        .expect("恢复运行时必须成功");
    assert_eq!(summary_restored.state, RuntimeLifecycleStateDto::Running);
    assert!(!summary_restored.paused);

    // 7. get_diagnostics
    let diagnostics = service
        .get_diagnostics(EmptyPayload {})
        .await
        .expect("获取诊断必须成功");
    assert!(!diagnostics.components.is_empty());
    assert!(!diagnostics.items.is_empty());

    // 8. get_update_status：假传输固定返回与当前版本相同的 Release，状态必须是 UpToDate。
    let update_status = service
        .get_update_status(EmptyPayload {})
        .await
        .expect("获取更新状态必须成功");
    assert_eq!(update_status.state, UpdateStateDto::UpToDate);
    assert_eq!(update_status.current_version, env!("CARGO_PKG_VERSION"));
    assert_eq!(update_status.available_version, None);
    assert!(!update_status.preview);
    assert!(
        update_status.message.contains("最新"),
        "状态文案必须明确说明已是最新版本：{}",
        update_status.message
    );

    // 9. install_update：没有新版本时必须返回 UpToDate，而不是假装安装成功。
    let install_result = service
        .install_update(InstallUpdatePayload {})
        .await
        .expect("安装命令必须返回结果 DTO");
    assert_eq!(install_result.state, UpdateStateDto::UpToDate);
    assert_eq!(install_result.installed_version, None);
    assert!(!install_result.preview);

    // 10. quit_app
    let quit_res = service
        .quit_app(EmptyPayload {})
        .await
        .expect("退出必须成功");
    assert!(quit_res.accepted);
}

/// 契约测试专用的假更新传输：只回答"已是最新版本"，绝不访问真实网络。
struct UpToDateTransport;

#[async_trait::async_trait]
impl UpdateTransport for UpToDateTransport {
    async fn get(
        &self,
        _url: &str,
        _accept: &str,
        _limit: u64,
    ) -> Result<HttpTextResponse, UpdateError> {
        Ok(HttpTextResponse {
            final_url: String::new(),
            body:
                br#"{"tag_name":"v2.0.0","body":"","draft":false,"prerelease":false,"assets":[]}"#
                    .to_vec(),
        })
    }

    async fn download(
        &self,
        _url: &str,
        _destination: &std::path::Path,
        _limit: u64,
    ) -> Result<(), UpdateError> {
        Err(UpdateError::new(
            "update_test_unexpected_download",
            "已是最新版本时不应下载任何文件",
        ))
    }
}

/// 生产组合根注册五个适配器：id 稳定有序、能力与各自 crate 定义一致、新适配器默认不启用。
#[tokio::test]
async fn bootstrap_registers_all_agent_adapters_with_conservative_defaults() {
    let (_root, paths, store, secret_store) = create_test_env("agentnotify-agents-test-");

    let (_coordinator, service) = bootstrap_headless(paths, secret_store)
        .await
        .expect("Headless 装配与启动必须成功");

    let agents = service
        .list_agents(EmptyPayload {})
        .await
        .expect("列出 agents 必须成功");
    assert_eq!(
        agent_ids(&agents),
        EXPECTED_AGENT_IDS,
        "注册表必须按 Agent ID 稳定有序"
    );

    for agent in &agents {
        assert!(
            agent.capabilities.notify
                && agent.capabilities.resume
                && agent.capabilities.session_title
                && agent.capabilities.hook_installer,
            "{} 必须支持通知、续聊、会话标题与 Hook 安装",
            agent.id
        );
        assert!(!agent.display_name.is_empty(), "{}", agent.id);
        // 只有 Command Code 的续聊受回复窗口约束，与 crate 的 capabilities() 一致。
        assert_eq!(
            agent.capabilities.reply_window,
            agent.id == "commandcode",
            "{}",
            agent.id
        );
    }

    // descriptor 的展示名来自各自 crate，界面按注册表展示。
    for (agent_id, display_name) in [
        ("antigravity", "Antigravity"),
        ("codex", "Codex"),
        ("commandcode", "CommandCode"),
        ("devin", "Devin"),
        ("opencode", "OpenCode"),
    ] {
        assert_eq!(
            find_agent(&agents, agent_id).display_name,
            display_name,
            "{agent_id}"
        );
    }

    // 保守默认：新适配器必须先默认关闭，只有 OpenCode 保持“无配置行即启用”的历史行为。
    for agent in &agents {
        assert_eq!(
            agent.enabled,
            agent.id == "opencode",
            "{} 不得默认启用",
            agent.id
        );
    }

    // 补齐形状：新适配器有 enabled=false 的空配置行，OpenCode 不补行。
    let configs = store
        .agent_configs()
        .await
        .expect("查询 Agent 配置必须成功");
    assert!(
        !configs.contains_key("opencode"),
        "OpenCode 不补配置行，保持无行即启用的旧行为"
    );
    for agent_id in NEW_AGENT_IDS {
        let record = configs
            .get(agent_id)
            .unwrap_or_else(|| panic!("{agent_id} 必须补齐默认关闭的配置行"));
        assert!(!record.enabled, "{agent_id} 必须默认关闭");
        assert_eq!(record.config, serde_json::json!({}), "{agent_id}");
    }

    let _ = service.quit_app(EmptyPayload {}).await;
}

/// 补齐逻辑只插入缺失行：迁移写入的关闭状态与用户改过的配置都必须原样保留。
#[tokio::test]
async fn bootstrap_never_overwrites_existing_agent_configs() {
    let (_root, paths, store, secret_store) = create_test_env("agentnotify-agents-preserve-test-");

    // 模拟迁移按 devin.off 写入的关闭行，以及用户在界面里启用的 Codex 配置。
    store
        .upsert_agent_config("devin", false, &serde_json::json!({}))
        .await
        .expect("写入 Devin 关闭行必须成功");
    store
        .upsert_agent_config(
            "codex",
            true,
            &serde_json::json!({ "codexHome": r"D:\custom-codex" }),
        )
        .await
        .expect("写入 Codex 配置必须成功");
    let before = store
        .agent_configs()
        .await
        .expect("查询 Agent 配置必须成功");

    let (_coordinator, service) = bootstrap_headless(paths, secret_store)
        .await
        .expect("Headless 装配与启动必须成功");

    let agents = service
        .list_agents(EmptyPayload {})
        .await
        .expect("列出 agents 必须成功");
    assert!(
        !find_agent(&agents, "devin").enabled,
        "Devin 现有配置为关闭时不得被改回启用"
    );
    let codex = find_agent(&agents, "codex");
    assert!(codex.enabled, "用户显式启用的 Codex 不得被补齐逻辑关掉");
    assert_eq!(
        codex.config,
        serde_json::json!({ "codexHome": r"D:\custom-codex" })
    );

    let after = store
        .agent_configs()
        .await
        .expect("重新查询 Agent 配置必须成功");
    assert_eq!(
        after.get("devin"),
        before.get("devin"),
        "已有行必须原样保留"
    );
    assert_eq!(
        after.get("codex"),
        before.get("codex"),
        "已有行必须原样保留"
    );
    assert!(
        after.contains_key("antigravity") && after.contains_key("commandcode"),
        "缺失的配置行仍要补齐"
    );
    assert!(!after["commandcode"].enabled);

    let _ = service.quit_app(EmptyPayload {}).await;
}

/// 从旧版升级：旧版开着的 Agent（没有 marker）必须继承为启用，
/// 补齐逻辑在迁移之后也不得把它们改回关闭。
#[tokio::test]
async fn bootstrap_inherits_legacy_agent_switches_for_upgrades() {
    let (_root, paths, store, secret_store) = create_test_env("agentnotify-agents-legacy-test-");

    // 旧版遗留：config.json 存在、没有任何 marker，等价于旧版把五个 Agent 都开着。
    tokio::fs::write(paths.config_dir.join("config.json"), b"{}")
        .await
        .expect("写入旧版配置必须成功");

    let (_coordinator, service) = bootstrap_headless(paths, secret_store)
        .await
        .expect("Headless 装配与启动必须成功");

    let agents = service
        .list_agents(EmptyPayload {})
        .await
        .expect("列出 agents 必须成功");
    assert_eq!(agent_ids(&agents), EXPECTED_AGENT_IDS);
    for agent in &agents {
        assert!(agent.enabled, "{} 必须继承旧版的启用状态", agent.id);
    }

    let configs = store
        .agent_configs()
        .await
        .expect("查询 Agent 配置必须成功");
    assert_eq!(configs.len(), EXPECTED_AGENT_IDS.len());
    for (agent_id, record) in &configs {
        assert!(record.enabled, "{agent_id} 不得被补齐逻辑关掉");
        assert_eq!(record.config, serde_json::json!({}), "{agent_id}");
    }

    let _ = service.quit_app(EmptyPayload {}).await;
}

/// 新适配器的回复收件箱必须落在隔离根下：写入隔离根的新鲜心跳只会被隔离根上的适配器读到，
/// 真实用户目录（`%USERPROFILE%\.config\agent-notify`）不参与、也不会被写入。
#[tokio::test]
async fn new_agent_reply_inboxes_are_isolated_under_app_paths() {
    let (_root, paths, _store, secret_store) =
        create_test_env("agentnotify-agents-isolation-test-");

    let (coordinator, service) = bootstrap_headless(paths.clone(), secret_store)
        .await
        .expect("Headless 装配与启动必须成功");

    // 注册阶段只构造对象：Opencode / Devin 的收件箱在用到之前不得被创建。
    // Command Code 例外：启动装配会写窗口文件（应用自己的数据），但不会创建心跳等目录。
    for inbox_dir in ["opencode-reply-inbox", "devin-reply-inbox"] {
        assert!(
            !paths.config_dir.join(inbox_dir).exists(),
            "注册阶段不得创建收件箱目录 {inbox_dir}"
        );
    }
    let commandcode_inbox = paths.config_dir.join("commandcode-reply-inbox");
    assert!(
        commandcode_inbox
            .join(agentnotify_agent_commandcode::WINDOW_FILE_NAME)
            .is_file(),
        "启动装配必须写出 Command Code 窗口文件"
    );
    for created_dir in ["pending", "processing", "results", "heartbeats"] {
        assert!(
            !commandcode_inbox.join(created_dir).exists(),
            "启动装配不得创建 Command Code 运行目录 {created_dir}"
        );
    }

    // 隔离根里的新鲜心跳 ready=false：只有读到隔离根才会得到 devin_desktop_unsupported；
    // 若仍然读真实用户目录（本机与 CI 上都没有新鲜心跳），会得到
    // devin_extension_not_running 或 devin_extension_offline。
    let devin_heartbeats = paths
        .config_dir
        .join("devin-reply-inbox")
        .join("heartbeats");
    std::fs::create_dir_all(&devin_heartbeats).expect("创建隔离心跳目录必须成功");
    std::fs::write(
        devin_heartbeats.join("isolation-probe.json"),
        serde_json::json!({
            "ready": false,
            "timestamp": Timestamp::now_utc().to_rfc3339(),
        })
        .to_string(),
    )
    .expect("写入隔离心跳必须成功");

    let devin = coordinator
        .agent_registry()
        .get(&AgentId::new("devin").expect("固定有效标识"))
        .expect("Devin 适配器必须已注册");
    let error = devin
        .resume(
            &AgentSessionId::new("isolation-probe-session").expect("固定有效标识"),
            "引用回复探针",
        )
        .await
        .expect_err("隔离心跳 ready=false 时必须明确报错");
    assert_eq!(
        error.code(),
        "devin_desktop_unsupported",
        "Devin 收件箱必须指向隔离根，而不是真实用户目录"
    );

    // 支持性证据：隔离根里的新鲜心跳让 Command Code 的接入状态变为可用。
    let commandcode_heartbeats = paths
        .config_dir
        .join("commandcode-reply-inbox")
        .join("heartbeats");
    std::fs::create_dir_all(&commandcode_heartbeats).expect("创建隔离心跳目录必须成功");
    std::fs::write(
        commandcode_heartbeats.join("isolation-probe.json"),
        serde_json::json!({
            "ready": true,
            "timestamp": Timestamp::now_utc().to_rfc3339(),
            "sessionId": "isolation-probe-session",
            "windowOpen": true,
        })
        .to_string(),
    )
    .expect("写入隔离心跳必须成功");

    let agents = service
        .list_agents(EmptyPayload {})
        .await
        .expect("列出 agents 必须成功");
    assert!(
        find_agent(&agents, "commandcode").health.available,
        "Command Code 适配器必须读到隔离根里的心跳"
    );

    let _ = service.quit_app(EmptyPayload {}).await;
}

/// 界面里保存的 Agent 配置必须作用到适配器，`update_agent_config` 后立即生效。
#[tokio::test]
async fn saved_agent_config_reaches_adapters_and_update_takes_effect() {
    let (_root, paths, store, secret_store) =
        create_test_env("agentnotify-agent-config-live-test-");

    let first_home = tempfile::Builder::new()
        .prefix("agentnotify-codex-home-first-")
        .tempdir_in(agentnotify_testkit::test_temp_root())
        .expect("测试临时目录必须可创建");
    let second_home = tempfile::Builder::new()
        .prefix("agentnotify-codex-home-second-")
        .tempdir_in(agentnotify_testkit::test_temp_root())
        .expect("测试临时目录必须可创建");
    write_codex_state_database(first_home.path(), "thread-1", "状态库标题");
    write_codex_session_index(second_home.path(), "thread-1", "更新后的标题");

    store
        .upsert_agent_config(
            "codex",
            true,
            &serde_json::json!({"codexHome": first_home.path()}),
        )
        .await
        .expect("写入 Codex 配置必须成功");

    let (coordinator, service) = bootstrap_headless(paths, secret_store)
        .await
        .expect("Headless 装配与启动必须成功");

    assert_eq!(
        codex_title(&coordinator, "thread-1"),
        "状态库标题",
        "标题解析必须读取配置 codexHome 下的状态库"
    );

    service
        .update_agent_config(UpdateAgentConfigPayload {
            agent_id: "codex".into(),
            enabled: Some(true),
            config: Some(serde_json::json!({"codexHome": second_home.path()})),
        })
        .await
        .expect("更新 Codex 配置必须成功");

    assert_eq!(
        codex_title(&coordinator, "thread-1"),
        "更新后的标题",
        "update_agent_config 之后必须用新配置重建适配器"
    );

    let _ = service.quit_app(EmptyPayload {}).await;
}

/// 界面保存的 Command Code 回复窗口必须到达 mod 能读到的 `window.json`：
/// 启动装配写一次，`update_agent_config` 保存后立即同步，两次都与适配器同源。
#[tokio::test]
async fn commandcode_reply_window_reaches_the_mod_window_file() {
    let (_root, paths, store, secret_store) =
        create_test_env("agentnotify-commandcode-window-test-");
    let window_file = paths
        .config_dir
        .join("commandcode-reply-inbox")
        .join(agentnotify_agent_commandcode::WINDOW_FILE_NAME);

    store
        .upsert_agent_config(
            "commandcode",
            true,
            &serde_json::json!({"commandCodeReplyWindowSec": 120}),
        )
        .await
        .expect("写入 CommandCode 配置必须成功");

    let (_coordinator, service) = bootstrap_headless(paths.clone(), secret_store)
        .await
        .expect("Headless 装配与启动必须成功");

    assert_eq!(
        window_file_sec(&window_file),
        120,
        "启动装配必须把界面里已有的窗口写给 mod"
    );

    let updated = service
        .update_agent_config(UpdateAgentConfigPayload {
            agent_id: "commandcode".into(),
            enabled: Some(true),
            config: Some(serde_json::json!({"commandCodeReplyWindowSec": 300})),
        })
        .await
        .expect("更新 CommandCode 配置必须成功");
    assert_eq!(
        updated.config["commandCodeReplyWindowSec"], 300,
        "配置必须落库"
    );
    assert_eq!(
        window_file_sec(&window_file),
        300,
        "保存后必须立即把同一个值写给 mod"
    );
    assert!(
        window_file.starts_with(&paths.config_dir),
        "窗口文件必须落在应用自己的收件箱里：{}",
        window_file.display()
    );

    let _ = service.quit_app(EmptyPayload {}).await;
}

/// 无效配置必须先报错、绝不落库：否则带着无效行重启会让宿主起不来（用户被关在门外）。
#[tokio::test]
async fn invalid_agent_config_is_rejected_without_persisting() {
    let (_root, paths, store, secret_store) =
        create_test_env("agentnotify-agent-config-invalid-test-");

    store
        .upsert_agent_config("codex", true, &serde_json::json!({}))
        .await
        .expect("写入基线配置必须成功");
    let before = store.agent_configs().await.expect("读取基线配置必须成功");

    let (_coordinator, service) = bootstrap_headless(paths, secret_store)
        .await
        .expect("Headless 装配与启动必须成功");

    let error = service
        .update_agent_config(UpdateAgentConfigPayload {
            agent_id: "codex".into(),
            enabled: Some(true),
            config: Some(serde_json::json!({"codexHome": "relative/path"})),
        })
        .await
        .expect_err("相对路径属于无效配置，必须明确报错");
    assert_eq!(error.code(), "agent_config_invalid");

    let after = store.agent_configs().await.expect("读取配置必须成功");
    // 启动时会为未配置的 Agent 补默认关闭行，所以只断言 codex 行保持原样、且无效值没落库。
    assert_eq!(
        after.get("codex"),
        before.get("codex"),
        "无效配置绝不能写入数据库（codex 行必须保持原样）"
    );
    let leaked = after
        .values()
        .any(|record| record.config.to_string().contains("relative/path"));
    assert!(!leaked, "无效值绝不能出现在数据库里");

    let _ = service.quit_app(EmptyPayload {}).await;
}

/// `replyInbox` 配置必须作用到 Devin 适配器：配置路径优先于 AppPaths 默认收件箱。
#[tokio::test]
async fn configured_devin_reply_inbox_overrides_the_app_paths_inbox() {
    let (_root, paths, store, secret_store) =
        create_test_env("agentnotify-agent-config-inbox-test-");
    let configured_inbox = tempfile::Builder::new()
        .prefix("agentnotify-devin-inbox-")
        .tempdir_in(agentnotify_testkit::test_temp_root())
        .expect("测试临时目录必须可创建");
    let heartbeats = configured_inbox.path().join("heartbeats");
    std::fs::create_dir_all(&heartbeats).expect("创建心跳目录必须成功");
    std::fs::write(
        heartbeats.join("config-probe.json"),
        serde_json::json!({
            "ready": false,
            "timestamp": Timestamp::now_utc().to_rfc3339(),
        })
        .to_string(),
    )
    .expect("写入隔离心跳必须成功");

    store
        .upsert_agent_config(
            "devin",
            true,
            &serde_json::json!({"replyInbox": configured_inbox.path()}),
        )
        .await
        .expect("写入 Devin 配置必须成功");

    let (coordinator, service) = bootstrap_headless(paths.clone(), secret_store)
        .await
        .expect("Headless 装配与启动必须成功");

    let devin = coordinator
        .agent_registry()
        .get(&AgentId::new("devin").expect("固定有效标识"))
        .expect("Devin 适配器必须已注册");
    let error = devin
        .resume(
            &AgentSessionId::new("config-probe-session").expect("固定有效标识"),
            "配置探针",
        )
        .await
        .expect_err("配置心跳 ready=false 时必须明确报错");
    assert_eq!(
        error.code(),
        "devin_desktop_unsupported",
        "Devin 收件箱必须指向配置路径"
    );
    assert!(
        !paths.config_dir.join("devin-reply-inbox").exists(),
        "配置了收件箱时不应读取或创建 AppPaths 默认收件箱"
    );

    let _ = service.quit_app(EmptyPayload {}).await;
}

/// 读取 mod 窗口文件里的秒数；文件缺失或格式不对直接失败，避免测试静默放过。
fn window_file_sec(path: &std::path::Path) -> u64 {
    let content = std::fs::read_to_string(path)
        .unwrap_or_else(|error| panic!("窗口文件必须存在 {}：{error}", path.display()));
    let parsed: serde_json::Value =
        serde_json::from_str(&content).expect("窗口文件必须是合法 JSON");
    parsed["commandCodeReplyWindowSec"]
        .as_u64()
        .expect("窗口秒数必须是非负整数")
}

/// 用注册表里的 Codex 适配器解析一条完成事件的标题。
fn codex_title(coordinator: &ProductionRuntimeCoordinator, thread_id: &str) -> String {
    let adapter = coordinator
        .agent_registry()
        .get(&AgentId::new("codex").expect("固定有效标识"))
        .expect("Codex 适配器必须已注册");
    let event = agentnotify_agent_sdk::AgentEventEnvelope {
        request_id: agentnotify_domain::RequestId::new("req-codex-config-test")
            .expect("固定有效标识"),
        agent_id: AgentId::new("codex").expect("固定有效标识"),
        payload: serde_json::json!({
            "thread-id": thread_id,
            "last-assistant-message": "done"
        }),
    };
    adapter
        .parse_event(event)
        .expect("Codex 完成事件必须可解析")
        .title
}

fn write_codex_session_index(home: &std::path::Path, thread_id: &str, title: &str) {
    std::fs::write(
        home.join("session_index.jsonl"),
        format!("{{\"id\":\"{thread_id}\",\"thread_name\":\"{title}\"}}\n"),
    )
    .expect("写入 Codex 会话索引必须成功");
}

/// 造一个最小可用的 Codex 状态库：`state_*.sqlite` 里的 `threads.name` 是标题链首级。
fn write_codex_state_database(home: &std::path::Path, thread_id: &str, title: &str) {
    let connection =
        rusqlite::Connection::open(home.join("state_5.sqlite")).expect("创建 Codex 状态库必须成功");
    connection
        .execute_batch("CREATE TABLE threads (id TEXT PRIMARY KEY, name TEXT);")
        .expect("创建 threads 表必须成功");
    connection
        .execute(
            "INSERT INTO threads (id, name) VALUES (?1, ?2)",
            [thread_id, title],
        )
        .expect("写入 Codex 线程标题必须成功");
}

#[tokio::test]
async fn runtime_restarts_repeatedly_and_maintains_running_state() {
    let (_root, paths, _store, secret_store) = create_test_env("agentnotify-runtime-restart-test-");

    let (coordinator, service) = bootstrap_headless(paths, secret_store)
        .await
        .expect("Headless 装配与启动必须成功");

    // 1. 验证初始状态稳定进入 Running (通过宿主服务快照与内部 is_running 断言)
    let mut initial_running = false;
    for _ in 0..30 {
        let host_snapshot = service
            .get_snapshot(EmptyPayload {})
            .await
            .expect("获取快照成功");
        if host_snapshot.runtime.state == RuntimeLifecycleStateDto::Running {
            initial_running = true;
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    assert!(initial_running, "初次启动必须进入 Running 状态");

    // 2. 连续 3 次调用 start_or_restart，断言独占锁正常释放与交接，绝不降级进入 MigrationRequired
    for i in 1..=3 {
        let snapshot = coordinator
            .start_or_restart()
            .await
            .unwrap_or_else(|e| panic!("第 {i} 次 start_or_restart 必须成功，但得到错误: {e:?}"));

        assert_ne!(
            snapshot.migration.state,
            MigrationState::Required,
            "第 {i} 次重启决不能降级进入 MigrationRequired 诊断模式"
        );

        // 等待刷新并验证宿主快照与内部状态均为 Running
        let mut is_running = false;
        for _ in 0..30 {
            let host_snapshot = service
                .get_snapshot(EmptyPayload {})
                .await
                .expect("获取宿主快照成功");
            if host_snapshot.runtime.state == RuntimeLifecycleStateDto::Running {
                is_running = true;
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
        assert!(is_running, "第 {i} 次重启后 runtime 状态必须仍为 Running");

        let current_core = coordinator.current_snapshot().await.expect("快照必须存在");
        assert!(
            current_core.state.is_running(),
            "第 {i} 次重启后内部核心状态必须处于 Running 或就绪状态"
        );
        assert_ne!(
            current_core.state,
            RuntimeState::MigrationRequired,
            "第 {i} 次重启后内部核心状态绝不能是 MigrationRequired"
        );
    }

    // 3. 通过 update_settings 修改需要重启 runtime 的字段 (如 reply_enabled)
    let settings = service
        .get_settings(EmptyPayload {})
        .await
        .expect("获取设置成功");
    let mut modified_settings = settings.clone();
    modified_settings.reply_enabled = !settings.reply_enabled;

    let _ = service
        .update_settings(modified_settings)
        .await
        .expect("更新设置并触发内部重启必须成功");

    let mut settings_restart_running = false;
    for _ in 0..30 {
        let host_snapshot = service
            .get_snapshot(EmptyPayload {})
            .await
            .expect("获取宿主快照成功");
        if host_snapshot.runtime.state == RuntimeLifecycleStateDto::Running {
            settings_restart_running = true;
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    assert!(
        settings_restart_running,
        "设置更新重启后 runtime 状态必须仍为 Running"
    );

    let current = coordinator
        .current_snapshot()
        .await
        .expect("当前快照必须存在");
    assert_ne!(
        current.migration.state,
        MigrationState::Required,
        "设置更新重启后决不能降级进入 MigrationRequired 诊断模式"
    );
    assert_ne!(
        current.state,
        RuntimeState::MigrationRequired,
        "设置更新重启后核心状态决不能降级为 MigrationRequired"
    );

    // 4. 清理优雅退出
    let _ = service.quit_app(EmptyPayload {}).await;
}

#[tokio::test]
async fn quit_app_checkpoints_wal_and_stops_runtime() {
    let (_root, paths, store, secret_store) = create_test_env("agentnotify-quit-checkpoint-test-");
    let (coordinator, service) = bootstrap_headless(paths.clone(), secret_store)
        .await
        .expect("Headless 装配与启动必须成功");

    let notification_id = NotificationId::new("notif-quit-1").unwrap();
    let notification = Notification::new(
        notification_id.clone(),
        "event-quit-1".to_string(),
        AgentId::new("opencode").unwrap(),
        Some(AgentSessionId::new("session-quit-1").unwrap()),
        Some("退出检查点".to_string()),
        "退出检查点",
        "正文",
        Timestamp::now_utc(),
        NotificationMetadata::default(),
    )
    .unwrap();
    store
        .commit_ingest(
            notification,
            vec![OutboxItem::pending(
                "outbox-quit-1".to_string(),
                notification_id,
                Timestamp::now_utc(),
            )],
        )
        .await
        .expect("提交通知记录成功");

    let wal_path = paths.data_dir.join("state.db-wal");
    assert!(wal_path.exists(), "运行时持锁期间应存在 WAL 文件");

    service
        .quit_app(EmptyPayload {})
        .await
        .expect("退出命令必须成功");

    // Headless 退出必须同步完成运行时关闭与 WAL checkpoint，回滚窗口依赖这一语义。
    let wal_len = std::fs::metadata(&wal_path)
        .map(|meta| meta.len())
        .unwrap_or(0);
    assert_eq!(wal_len, 0, "退出后 WAL 必须被截断");
    assert!(
        store.integrity_check().await.unwrap(),
        "退出后数据库必须完整"
    );
    assert!(
        coordinator.current_snapshot().await.is_none(),
        "退出后运行时不应仍然可访问"
    );
}

#[tokio::test]
async fn retry_delivery_reloads_and_returns_latest_delivery_record() {
    let (_root, paths, store, secret_store) = create_test_env("agentnotify-delivery-retry-test-");

    let (_coordinator, service) = bootstrap_headless(paths, secret_store)
        .await
        .expect("Headless 装配与启动必须成功");

    // 本用例直接租约 Outbox 验证 retry_delivery；先关闭运行时，避免后台 worker 抢先租约造成偶发失败。
    _coordinator
        .shutdown_runtime()
        .await
        .expect("关闭运行时必须成功");

    // 构造通知与失败的可重试投递记录
    let notification_id = NotificationId::new("notif-retry-1").unwrap();
    let notification = Notification::new(
        notification_id.clone(),
        "event-retry-1".to_string(),
        AgentId::new("opencode").unwrap(),
        Some(AgentSessionId::new("session-retry-1").unwrap()),
        Some("会话重试".to_string()),
        "重试测试",
        "正文",
        Timestamp::now_utc(),
        NotificationMetadata::default(),
    )
    .unwrap();
    store
        .commit_ingest(
            notification.clone(),
            vec![OutboxItem::pending(
                "outbox-retry-1".to_string(),
                notification_id.clone(),
                Timestamp::now_utc(),
            )],
        )
        .await
        .expect("提交通知记录成功");

    let delivery_id = DeliveryId::new("deliv-retry-1").unwrap();
    let mut delivery = Delivery::pending(
        delivery_id.clone(),
        notification_id.clone(),
        ChannelId::new(CLAWBOT_CHANNEL_ID).unwrap(),
        ChannelAccountId::new("acc-test").unwrap(),
    );
    delivery
        .mark_retryable(SafeError::new("network_timeout", "超时网络错误").unwrap())
        .unwrap();

    let lease = store
        .lease_next_outbox(
            Timestamp::now_utc(),
            Timestamp::parse_rfc3339("2030-01-01T00:00:00Z").unwrap(),
        )
        .await
        .unwrap()
        .unwrap();
    store
        .commit_delivery(lease, delivery.clone(), None)
        .await
        .expect("提交失败投递记录成功");

    // 执行 retry_delivery，应重新排队并成功重新查询到最新投递记录
    let retried = service
        .retry_delivery(DeliveryIdPayload {
            delivery_id: delivery_id.to_string(),
        })
        .await
        .expect("重试投递必须成功返回最新记录");

    assert_eq!(retried.id, delivery_id.to_string());
    assert_eq!(retried.channel_id, CLAWBOT_CHANNEL_ID);
    assert_eq!(retried.account_id, "acc-test");

    let _ = service.quit_app(EmptyPayload {}).await;
}

#[tokio::test]
async fn logout_channel_account_surfaces_error_when_channel_fails_or_account_missing() {
    let (_root, paths, _store, secret_store) = create_test_env("agentnotify-logout-test-");

    let (_coordinator, service) = bootstrap_headless(paths, secret_store)
        .await
        .expect("Headless 装配与启动必须成功");

    // 对不存在的账号执行登出，必须明确报错，不能静默成功
    let result = service
        .logout_channel_account(ChannelAccountIdPayload {
            account_id: "non-existent-account".into(),
        })
        .await;

    assert!(result.is_err(), "登出不存在的账号必须返回错误");
    let error = result.unwrap_err();
    assert_eq!(error.code, "account_not_found");

    let _ = service.quit_app(EmptyPayload {}).await;
}
#[tokio::test]
async fn target_provider_uses_latest_established_account_as_compatible_fallback() {
    let (_root, paths, store, secret_store) = create_test_env("agentnotify-target-fallback-test-");
    let settings_store = ProductionSettingsStore::new(store.clone(), &paths.config_dir);

    let mut agent_registry = agentnotify_agent_sdk::AgentRegistry::default();
    let opencode_inbox = agentnotify_agent_opencode::OpenCodeReplyInbox::new(
        paths.config_dir.join("opencode-reply-inbox"),
    );
    agent_registry
        .register(Arc::new(agentnotify_agent_opencode::OpenCodeAgent::new(
            opencode_inbox,
        )))
        .expect("注册 agent 必须成功");

    let clawbot_channel = agentnotify_channel_clawbot::ClawBotChannel::new(secret_store.clone())
        .with_account_store(store.clone());
    let mut channel_registry = agentnotify_channel_sdk::ChannelRegistry::default();
    channel_registry
        .register(Arc::new(clawbot_channel))
        .expect("注册 channel 必须成功");

    let target_provider = ProductionTargetProvider::new(
        store.clone(),
        settings_store.clone(),
        secret_store.clone(),
        Arc::new(agent_registry),
        Arc::new(channel_registry),
    );

    let older = make_clawbot_account("test-bot-old", "wx-user-old", "2026-09-19T01:00:00Z");
    let newer = make_clawbot_account("test-bot-new", "wx-user-new", "2026-09-20T01:00:00Z");
    let older_id = older.id.clone();
    let newer_id = newer.id.clone();

    for account in [older, newer] {
        let credentials = serde_json::json!({
            "bot_token": format!("token-{}", account.id),
            "bot_id": format!("bot-{}", account.id),
            "user_id": account.config["user_id_hint"].clone(),
            "base_url": "https://ilinkai.weixin.qq.com"
        });
        let account_id = account.id.clone();
        store.upsert(account).await.expect("upsert 账号必须成功");
        secret_store
            .set(
                &account_id,
                SecretKind::BotToken,
                SecretValue::new(credentials.to_string()).expect("凭据必须有效"),
            )
            .await
            .expect("写入密钥必须成功");
    }

    let resolved = target_provider.resolve().await.expect("解析目标必须成功");
    assert_eq!(resolved.delivery_targets.len(), 2);
    assert_eq!(
        resolved.delivery_targets[0].account.id, newer_id,
        "未显式选择时必须优先最近建立会话的账号"
    );

    let mut settings = settings_store
        .load_settings()
        .await
        .expect("读取设置必须成功");
    settings.default_channel_account_id = Some(older_id.to_string());
    settings_store
        .save_settings(&settings)
        .await
        .expect("保存默认账号必须成功");

    let resolved = target_provider
        .resolve()
        .await
        .expect("重新解析目标必须成功");
    assert_eq!(
        resolved.delivery_targets[0].account.id, older_id,
        "显式默认账号必须优先于会话新旧"
    );
}

#[tokio::test]
async fn list_channel_accounts_reports_stale_and_missing_credentials() {
    let (_root, paths, store, secret_store) = create_test_env("agentnotify-channel-health-test-");
    let (_coordinator, service) = bootstrap_headless(paths, secret_store.clone())
        .await
        .expect("Headless 装配与启动必须成功");

    // 1. 凭据仍在但会话已失效：健康状态必须 stale，并带可操作提示。
    let mut stale = make_clawbot_account("test-bot-stale", "wx-user-stale", "2026-09-19T01:00:00Z");
    stale.config["base_url"] = serde_json::json!("http://127.0.0.1:9");
    stale.config["stale_at"] =
        serde_json::to_value(Timestamp::now_utc()).expect("时间必须可序列化");
    let stale_id = stale.id.clone();
    store.upsert(stale).await.expect("upsert 会话失效账号成功");
    secret_store
        .set(
            &stale_id,
            SecretKind::BotToken,
            SecretValue::new(
                serde_json::json!({
                    // 指向本机未监听端口，避免测试期间访问真实平台。
                    "bot_token": "test-bot-token",
                    "bot_id": "test-bot-stale",
                    "user_id": "wx-user-stale",
                    "base_url": "http://127.0.0.1:9"
                })
                .to_string(),
            )
            .expect("凭据必须有效"),
        )
        .await
        .expect("写入密钥成功");

    // 2. 完全没有凭据：健康状态必须不可用，并提示重新扫码。
    let missing = make_clawbot_account(
        "test-bot-missing",
        "wx-user-missing",
        "2026-09-19T01:00:00Z",
    );
    let missing_id = missing.id.clone();
    store.upsert(missing).await.expect("upsert 无凭据账号成功");

    let channels = service
        .list_channel_accounts(EmptyPayload {})
        .await
        .expect("列出渠道账号必须成功");
    let clawbot = channels
        .channels
        .iter()
        .find(|channel| channel.id == CLAWBOT_CHANNEL_ID)
        .expect("必须包含 ClawBot 渠道");

    let stale_dto = clawbot
        .accounts
        .iter()
        .find(|account| account.id.as_str() == stale_id.as_str())
        .expect("必须包含会话失效账号");
    assert!(stale_dto.health.stale, "会话失效的账号必须标记 stale");
    let stale_detail = stale_dto
        .health
        .detail
        .as_ref()
        .expect("stale 账号必须带安全明细");
    assert_eq!(stale_detail.code, "clawbot_session_stale");
    assert!(
        stale_detail.message.contains("重新扫码") && stale_detail.message.contains("发送消息恢复"),
        "提示不可操作：{}",
        stale_detail.message
    );

    let missing_dto = clawbot
        .accounts
        .iter()
        .find(|account| account.id.as_str() == missing_id.as_str())
        .expect("必须包含无凭据账号");
    assert!(!missing_dto.health.available, "缺少凭据必须报告不可用");
    let missing_detail = missing_dto
        .health
        .detail
        .as_ref()
        .expect("不可用账号必须带安全明细");
    // 缺少凭据时渠道层给出的是“重新登录”，与 ret=-14 的“重新扫码”同属可操作提示。
    assert!(
        missing_detail.message.contains("重新登录") || missing_detail.message.contains("重新扫码"),
        "提示不可操作：{}",
        missing_detail.message
    );

    // 账号配置必须保持脱敏，不得出现任何凭据明文。
    for account in &clawbot.accounts {
        assert!(
            !account.config.to_string().contains("test-bot-token"),
            "渠道账号配置不得泄露凭据"
        );
    }

    let _ = service.quit_app(EmptyPayload {}).await;
}

fn make_clawbot_account(
    bot_id: &str,
    user_id: &str,
    established_at: &str,
) -> agentnotify_channel_sdk::ChannelAccount {
    let mut account =
        agentnotify_channel_clawbot::ClawBotAccount::from_platform_ids(bot_id, user_id)
            .expect("构造 ClawBot 账号成功")
            .into_channel_account()
            .expect("转为 ChannelAccount 成功");
    account.enabled = true;
    account.config["session_established_at"] =
        serde_json::to_value(Timestamp::parse_rfc3339(established_at).expect("时间必须有效"))
            .expect("时间必须可序列化");
    account
}
