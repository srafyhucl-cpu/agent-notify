use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};

use agentnotify_application::{
    ChannelAccountStore, SecretError, SecretKind, SecretStore, SecretValue,
};
use agentnotify_channel_clawbot::CLAWBOT_CHANNEL_ID;
use agentnotify_desktop::bridge::commands::HostCommandService;
use agentnotify_desktop::bridge::dto::*;
use agentnotify_desktop::platform::AppPaths;
use agentnotify_desktop::platform::windows::{CredentialBackend, WindowsSecretStore};
use agentnotify_desktop::production::{
    ProductionSettingsStore, ProductionTargetProvider, bootstrap_headless,
};
use agentnotify_runtime::RuntimeTargetProvider;
use agentnotify_storage_sqlite::SqliteStore;

const D_DRIVE_TEMP: &str = r"D:\Temp";

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
        .tempdir_in(D_DRIVE_TEMP)
        .expect("D 盘临时目录必须可创建");
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

    let (coordinator, service) = bootstrap_headless(paths, secret_store)
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
    assert_eq!(snapshot.overview.agents.len(), 1);
    assert_eq!(snapshot.overview.agents[0].id, "opencode");

    // 2. list_agents
    let agents = service
        .list_agents(EmptyPayload {})
        .await
        .expect("列出 agents 必须成功");
    assert_eq!(agents.len(), 1);
    assert_eq!(agents[0].id, "opencode");
    assert!(agents[0].enabled);

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

    // 8. get_update_status
    let update_status = service
        .get_update_status(EmptyPayload {})
        .await
        .expect("获取更新状态必须成功");
    assert_eq!(update_status.state, UpdateStateDto::Unsupported);
    assert_eq!(update_status.current_version, env!("CARGO_PKG_VERSION"));
}
