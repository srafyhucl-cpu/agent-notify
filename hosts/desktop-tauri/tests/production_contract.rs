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
    ProductionSettingsStore, ProductionTargetProvider, bootstrap_headless,
};
use agentnotify_domain::{
    AgentId, AgentSessionId, ChannelAccountId, ChannelId, Delivery, DeliveryId, Notification,
    NotificationId, NotificationMetadata, SafeError, Timestamp,
};
use agentnotify_runtime::{MigrationState, RuntimeState, RuntimeTargetProvider};
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

    // 9. quit_app
    let quit_res = service
        .quit_app(EmptyPayload {})
        .await
        .expect("退出必须成功");
    assert!(quit_res.accepted);
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
