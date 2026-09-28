use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};

use agentnotify_agent_sdk::{
    AgentAdapter, AgentCapabilities, AgentDescriptor, AgentError, AgentEventEnvelope, AgentHealth,
    AgentRegistry, NormalizedAgentEvent, ResumeReceipt,
};
use agentnotify_application::{
    AgentNotificationConfig, ChannelAccountStore, Clock, DeliveryTarget, IdGenerator, IngestStore,
    NotificationPolicy, OutboxItem, ReplyConfig,
};
use agentnotify_channel_sdk::{
    ChannelAccount, ChannelAdapter, ChannelCapabilities, ChannelDescriptor, ChannelError,
    ChannelHealth, ChannelRegistry, ChannelTask, DeliveryReceipt, InboundEmitter, InboundMode,
    OutboundMessage,
};
use agentnotify_domain::{
    AgentId, AgentSessionId, ChannelAccountId, ChannelId, DeliveryState, ExternalMessageId,
    Notification, NotificationId, NotificationMetadata, RequestId, SafeError, Timestamp,
};
use agentnotify_runtime::{AgentEventObserver, AppRuntime, RuntimeConfig};
use agentnotify_storage_sqlite::SqliteStore;
use tempfile::TempDir;
use tokio::sync::{Notify, watch};

struct FixedClock;

impl Clock for FixedClock {
    fn now(&self) -> Timestamp {
        Timestamp::parse_rfc3339("2026-09-19T10:00:00Z").unwrap()
    }
}

struct SequenceIds(AtomicU64);

impl SequenceIds {
    fn new() -> Self {
        Self(AtomicU64::new(1))
    }
}

impl IdGenerator for SequenceIds {
    fn next_id(&self) -> String {
        format!("generated-{}", self.0.fetch_add(1, Ordering::Relaxed))
    }
}

struct TestChannel {
    id: ChannelId,
    fail_start: bool,
    started: Notify,
    sent: AtomicU64,
}

impl TestChannel {
    fn new(id: &str, fail_start: bool) -> Self {
        Self {
            id: ChannelId::new(id).unwrap(),
            fail_start,
            started: Notify::new(),
            sent: AtomicU64::new(0),
        }
    }

    fn sent_count(&self) -> u64 {
        self.sent.load(Ordering::SeqCst)
    }

    async fn wait_started(&self) {
        tokio::time::timeout(Duration::from_secs(1), self.started.notified())
            .await
            .expect("渠道应在超时前启动");
    }
}

#[async_trait::async_trait]
impl ChannelAdapter for TestChannel {
    fn descriptor(&self) -> ChannelDescriptor {
        ChannelDescriptor {
            id: self.id.clone(),
            display_name: self.id.as_str().into(),
            config_schema: serde_json::json!({"type": "object"}),
        }
    }

    fn capabilities(&self) -> ChannelCapabilities {
        ChannelCapabilities {
            send_text: true,
            receive: true,
            reply_routing: true,
            edit_message: false,
            attachments: false,
            markdown: false,
            max_text_bytes: None,
            inbound_modes: vec![InboundMode::LongPolling],
        }
    }

    async fn start(
        &self,
        _account: ChannelAccount,
        _emit: InboundEmitter,
    ) -> Result<ChannelTask, ChannelError> {
        if self.fail_start {
            return Err(ChannelError::permanent(
                "channel_start_failed",
                "测试渠道启动失败",
            ));
        }
        self.started.notify_one();
        let (cancel, mut cancel_receiver) = watch::channel(false);
        let handle = tokio::spawn(async move {
            let _ = cancel_receiver.changed().await;
            Ok(())
        });
        Ok(ChannelTask::new(handle, cancel))
    }

    async fn send(
        &self,
        _account: ChannelAccount,
        _message: OutboundMessage,
    ) -> Result<DeliveryReceipt, ChannelError> {
        let sequence = self.sent.fetch_add(1, Ordering::SeqCst) + 1;
        Ok(DeliveryReceipt {
            external_message_id: Some(
                ExternalMessageId::new(format!("test-message-{sequence}")).unwrap(),
            ),
            external_thread_id: None,
            state: DeliveryState::Sent,
            error: None,
            raw_safe_metadata: Default::default(),
        })
    }

    async fn inspect(&self, _account: ChannelAccount) -> ChannelHealth {
        if self.fail_start {
            ChannelHealth::unavailable(
                SafeError::new("channel_start_failed", "测试渠道启动失败").unwrap(),
            )
        } else {
            ChannelHealth::healthy()
        }
    }

    async fn logout(&self, _account: ChannelAccount) -> Result<(), ChannelError> {
        Ok(())
    }
}

struct Fixture {
    _temp: TempDir,
    config: RuntimeConfig,
    healthy: Arc<TestChannel>,
    failing: Arc<TestChannel>,
}

async fn fixture() -> Fixture {
    let temp = tempfile::tempdir().unwrap();
    let database_path = temp.path().join("runtime.db");
    let store = SqliteStore::open(&database_path).unwrap();
    let account_store: Arc<dyn ChannelAccountStore> = Arc::new(store);
    let now = FixedClock.now();
    for (account_id, channel_id) in [
        ("healthy-account", "healthy"),
        ("failing-account", "failing"),
    ] {
        account_store
            .upsert(ChannelAccount::new(
                ChannelAccountId::new(account_id).unwrap(),
                ChannelId::new(channel_id).unwrap(),
                account_id,
                now,
            ))
            .await
            .unwrap();
    }

    let healthy = Arc::new(TestChannel::new("healthy", false));
    let failing = Arc::new(TestChannel::new("failing", true));
    let mut channels = ChannelRegistry::default();
    channels.register(healthy.clone()).unwrap();
    channels.register(failing.clone()).unwrap();

    Fixture {
        _temp: temp,
        config: RuntimeConfig {
            database_path,
            migration: None,
            agents: Arc::new(AgentRegistry::default()),
            channels: Arc::new(channels),
            clock: Arc::new(FixedClock),
            id_generator: Arc::new(SequenceIds::new()),
            notification_policy: NotificationPolicy::default(),
            delivery_targets: Vec::new(),
            reply_targets: Vec::new(),
            reply_config: ReplyConfig::default(),
            target_provider: None,
            app_version: "0.1.0-test".into(),
            platform: "windows".into(),
            ingress_spool_dir: None,
            ingress_pipe_enabled: false,
            telemetry: None,
            inbound_capacity: 16,
            worker_idle_delay: Duration::from_millis(5),
            status_refresh_interval: Duration::from_millis(5),
            channel_poll_interval: Duration::from_millis(5),
            spool_replay_interval: Duration::from_millis(5),
            inbound_interceptor: None,
            agent_event_observer: None,
        },
        healthy,
        failing,
    }
}

#[tokio::test]
async fn one_channel_failure_does_not_stop_other_tasks() {
    let Fixture {
        _temp,
        config,
        healthy,
        failing: _failing,
    } = fixture().await;
    let mut handle = AppRuntime::start(config).await.unwrap();
    healthy.wait_started().await;

    tokio::time::sleep(Duration::from_millis(30)).await;
    let snapshot = handle.snapshot();
    let runtime_state = handle.runtime_state();
    assert!(
        handle.is_running(),
        "单个渠道失败不能停止整个 runtime，当前状态: {runtime_state:?}，组件: {:?}",
        snapshot.components
    );
    assert!(snapshot.unhealthy_channel_count() >= 1);
    assert!(
        snapshot
            .components
            .iter()
            .any(|component| component.name.starts_with("channel:")
                && component.state == agentnotify_runtime::ComponentState::Failed)
    );

    handle.shutdown().await.unwrap();
    assert!(handle.store().integrity_check().await.unwrap());
}

#[tokio::test]
async fn shutdown_waits_for_outbox_checkpoint() {
    let Fixture { _temp, config, .. } = fixture().await;
    let mut handle = AppRuntime::start(config).await.unwrap();

    handle.shutdown().await.unwrap();

    assert!(!handle.is_running());
    assert!(handle.store().integrity_check().await.unwrap());
}

#[tokio::test]
async fn outbox_pause_holds_pending_delivery_until_resumed() {
    let Fixture {
        _temp,
        mut config,
        healthy,
        ..
    } = fixture().await;
    config.delivery_targets = vec![DeliveryTarget::new(
        ChannelAccount::new(
            ChannelAccountId::new("healthy-account").unwrap(),
            ChannelId::new("healthy").unwrap(),
            "健康测试账号",
            FixedClock.now(),
        ),
        "conversation-1",
    )];
    let mut handle = AppRuntime::start(config).await.unwrap();
    handle.set_outbox_paused(true);
    assert!(handle.outbox_paused());

    let notification = Notification::new(
        NotificationId::new("notification-paused").unwrap(),
        "pause-contract-1",
        AgentId::new("test-agent").unwrap(),
        None,
        None,
        "暂停测试",
        "暂停期间不得调用渠道",
        FixedClock.now(),
        NotificationMetadata::default(),
    )
    .unwrap();
    handle
        .store()
        .commit_ingest(
            notification.clone(),
            vec![OutboxItem::pending(
                "outbox-paused",
                notification.id.clone(),
                FixedClock.now(),
            )],
        )
        .await
        .unwrap();

    tokio::time::sleep(Duration::from_millis(30)).await;
    assert_eq!(healthy.sent_count(), 0, "暂停期间不得领取 Outbox");

    handle.set_outbox_paused(false);
    tokio::time::timeout(Duration::from_secs(1), async {
        while healthy.sent_count() == 0 {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("恢复后必须继续投递");
    assert_eq!(healthy.sent_count(), 1);

    handle.shutdown().await.unwrap();
}

/// 最小 Agent 适配器：接受 `session.completed` 事件（spool 周期重放测试用）。
struct ReplayAgent {
    id: AgentId,
}

#[async_trait::async_trait]
impl AgentAdapter for ReplayAgent {
    fn descriptor(&self) -> AgentDescriptor {
        AgentDescriptor {
            id: self.id.clone(),
            display_name: "Replay Agent".into(),
            description: "spool 重放测试 Agent".into(),
            config_schema: serde_json::json!({"type": "object"}),
        }
    }

    fn capabilities(&self) -> AgentCapabilities {
        AgentCapabilities {
            notify: true,
            resume: true,
            ..Default::default()
        }
    }

    fn parse_event(
        &self,
        envelope: AgentEventEnvelope,
    ) -> Result<NormalizedAgentEvent, AgentError> {
        if envelope.agent_id != self.id {
            return Err(AgentError::InvalidEvent);
        }
        let payload = &envelope.payload;
        let event_type = payload
            .get("eventType")
            .and_then(serde_json::Value::as_str)
            .ok_or(AgentError::InvalidEvent)?;
        if event_type != "session.completed" {
            return Err(AgentError::InvalidEvent);
        }
        let session_id = payload
            .get("sessionId")
            .and_then(serde_json::Value::as_str)
            .map(AgentSessionId::new)
            .transpose()
            .map_err(|_| AgentError::InvalidEvent)?;
        Ok(NormalizedAgentEvent {
            idempotency_key: payload
                .get("idempotencyKey")
                .and_then(serde_json::Value::as_str)
                .map(ToOwned::to_owned),
            occurred_at: payload
                .get("occurredAt")
                .and_then(serde_json::Value::as_str)
                .map(Timestamp::parse_rfc3339)
                .transpose()
                .map_err(|_| AgentError::InvalidEvent)?
                .ok_or(AgentError::InvalidEvent)?,
            session_id,
            session_title: None,
            title: payload
                .get("title")
                .and_then(serde_json::Value::as_str)
                .ok_or(AgentError::InvalidEvent)?
                .into(),
            body: payload
                .get("body")
                .and_then(serde_json::Value::as_str)
                .ok_or(AgentError::InvalidEvent)?
                .into(),
            metadata: NotificationMetadata::default(),
        })
    }

    async fn resume(
        &self,
        session_id: &AgentSessionId,
        _text: &str,
    ) -> Result<ResumeReceipt, AgentError> {
        Ok(ResumeReceipt {
            session_id: session_id.clone(),
        })
    }

    async fn inspect(&self) -> AgentHealth {
        AgentHealth::healthy()
    }
}

/// 记录观察者：断言 spool 重放事件确实送达回调。
#[derive(Default)]
struct RecordingObserver {
    seen: Mutex<Vec<String>>,
}

#[async_trait::async_trait]
impl AgentEventObserver for RecordingObserver {
    async fn observe(&self, envelope: &AgentEventEnvelope) {
        self.seen
            .lock()
            .expect("观察者记录锁")
            .push(envelope.request_id.to_string());
    }
}

/// 管道即时投递失败的事件会落盘在 spool；周期重放保证运行期就能补送（不必等下次启动）。
#[tokio::test]
async fn spool_replay_worker_delivers_events_without_restart() {
    let Fixture {
        _temp, mut config, ..
    } = fixture().await;
    let spool_dir = _temp.path().join("spool");
    std::fs::create_dir_all(&spool_dir).unwrap();
    config.ingress_spool_dir = Some(spool_dir.clone());

    let agent_id = AgentId::new("opencode").unwrap();
    let mut agents = AgentRegistry::default();
    agents
        .register(Arc::new(ReplayAgent {
            id: agent_id.clone(),
        }))
        .unwrap();
    config.agents = Arc::new(agents);
    config.notification_policy = NotificationPolicy::default()
        .with_agent(agent_id.clone(), AgentNotificationConfig::default());

    let observer = Arc::new(RecordingObserver::default());
    config.agent_event_observer = Some(observer.clone());

    let mut handle = AppRuntime::start(config).await.unwrap();

    // 运行期写入 spool，模拟 ingress 管道投递失败后的落盘事件。
    let spool =
        agentnotify_ingress::Spool::open(&spool_dir, agentnotify_ingress::SpoolLimits::default())
            .unwrap();
    let envelope = AgentEventEnvelope {
        request_id: RequestId::new("3f7c2b1a-9d4e-4f6a-8b5c-1e2d3f4a5b6c").unwrap(),
        agent_id: agent_id.clone(),
        payload: serde_json::json!({
            "eventType": "session.completed",
            "sessionId": "ses_spool_replay",
            "title": "重放测试",
            "body": "事件正文",
            "occurredAt": "2026-09-28T06:00:00Z",
        }),
    };
    spool.write_event(&envelope).unwrap();

    // fixture 的 spool_replay_interval = 5ms：等待周期重放送达观察者。
    let delivered = tokio::time::timeout(Duration::from_secs(2), async {
        while observer.seen.lock().expect("观察者锁").is_empty() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .is_ok();
    if !delivered {
        // 诊断输出：spool 里还剩什么（隔离目录 / 待重放文件）。
        eprintln!(
            "DEBUG spool queued={:?}",
            spool.queued_count().map_err(|e| format!("{e:?}"))
        );
        if let Ok(read_dir) = std::fs::read_dir(&spool_dir) {
            for entry in read_dir.flatten() {
                eprintln!("DEBUG spool entry: {:?}", entry.path());
            }
        }
        for sub in ["quarantine", "quarantined", "error"] {
            let dir = spool_dir.join(sub);
            if !dir.is_dir() {
                continue;
            }
            if let Ok(read_dir) = std::fs::read_dir(&dir) {
                for entry in read_dir.flatten() {
                    let path = entry.path();
                    if path.extension().and_then(std::ffi::OsStr::to_str) == Some("error") {
                        eprintln!(
                            "DEBUG {}: {} => {}",
                            sub,
                            path.display(),
                            std::fs::read_to_string(&path).unwrap_or_default()
                        );
                    } else {
                        eprintln!("DEBUG {}: {}", sub, path.display());
                    }
                }
            }
        }
    }
    assert!(delivered, "spool 周期重放应在超时前送达事件");
    assert_eq!(
        observer.seen.lock().expect("观察者锁").as_slice(),
        ["3f7c2b1a-9d4e-4f6a-8b5c-1e2d3f4a5b6c"]
    );
    assert_eq!(spool.queued_count().unwrap(), 0, "重放成功后 spool 应清空");

    handle.shutdown().await.unwrap();
}
