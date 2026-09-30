//! 微信集群指令路由端到端测试（P1-3）：真实 SQLite 文件上的
//! 「识别 → OrcCommandHandler 推进 → 微信回执」全链路，可无真微信（用消息解析 + 捕获渠道）。
//!
//! 覆盖：非集群消息零影响放行、指令/确认/恢复映射、blocked 语义、编排未启用、
//! 任务不存在、空正文等错误路径的中文回执，以及回执线程上下文（purpose/conversation/reply_to）。

use std::sync::{Arc, Mutex};

use agentnotify_application::ChannelAccountStore;
use agentnotify_channel_sdk::{
    ChannelAccount, ChannelAdapter, ChannelCapabilities, ChannelDescriptor, ChannelError,
    ChannelHealth, ChannelRegistry, ChannelTask, DeliveryReceipt, InboundEmitter, InboundMode,
    MessagePurpose, OutboundMessage,
};
use agentnotify_desktop::bridge::dto::{
    AdvanceOrcTaskPayload, CreateOrcTaskPayload, MarkBlockedOrcTaskPayload, OrcMessageKindDto,
    OrcTaskIdPayload, OrcTaskStateDto,
};
use agentnotify_desktop::production::WechatOrcRouter;
use agentnotify_desktop::production::orc_handler::OrcCommandHandler;
use agentnotify_domain::{
    ChannelAccountId, ChannelId, ExternalMessageId, InboundMessage, InboundMessageId,
    InboundMessageInput, Timestamp,
};
use agentnotify_orchestration::{OrcStore, Workflow};
use agentnotify_runtime::InboundInterceptor;
use agentnotify_storage_sqlite::SqliteStore;
use tempfile::TempDir;

const CHANNEL_ID: &str = "fake";
const ACCOUNT_ID: &str = "account-1";
const CONVERSATION_ID: &str = "conversation-1";

/// 捕获出站消息的测试渠道：记录每条发送，其余行为与 FakeChannel 一致。
struct CaptureChannel {
    sent: Arc<Mutex<Vec<OutboundMessage>>>,
}

impl CaptureChannel {
    fn new() -> (Arc<Self>, Arc<Mutex<Vec<OutboundMessage>>>) {
        let sent = Arc::new(Mutex::new(Vec::new()));
        (Arc::new(Self { sent: sent.clone() }), sent)
    }
}

#[async_trait::async_trait]
impl ChannelAdapter for CaptureChannel {
    fn descriptor(&self) -> ChannelDescriptor {
        ChannelDescriptor {
            id: ChannelId::new(CHANNEL_ID).unwrap(),
            display_name: "Capture Channel".into(),
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
            markdown: true,
            max_text_bytes: Some(1024),
            inbound_modes: vec![InboundMode::LongPolling],
        }
    }

    async fn start(
        &self,
        _account: ChannelAccount,
        _emit: InboundEmitter,
    ) -> Result<ChannelTask, ChannelError> {
        Ok(ChannelTask::completed())
    }

    async fn send(
        &self,
        _account: ChannelAccount,
        message: OutboundMessage,
    ) -> Result<DeliveryReceipt, ChannelError> {
        self.sent
            .lock()
            .expect("捕获渠道锁不应失败")
            .push(message.clone());
        Ok(DeliveryReceipt::sent(
            ExternalMessageId::new(format!("sent-{}", message.client_id)).unwrap(),
        ))
    }

    async fn inspect(&self, _account: ChannelAccount) -> ChannelHealth {
        ChannelHealth::healthy()
    }

    async fn logout(&self, _account: ChannelAccount) -> Result<(), ChannelError> {
        Ok(())
    }
}

/// 打开临时目录下的真实 SQLite 文件（落在 testkit 隔离根，避免 C 盘）。
fn open_sqlite(prefix: &str) -> (TempDir, Arc<SqliteStore>) {
    let root = tempfile::Builder::new()
        .prefix(prefix)
        .tempdir_in(agentnotify_testkit::test_temp_root())
        .expect("测试临时目录必须可创建");
    let store =
        Arc::new(SqliteStore::open(root.path().join("state.db")).expect("SQLite 数据库必须可创建"));
    (root, store)
}

fn test_account() -> ChannelAccount {
    ChannelAccount::new(
        ChannelAccountId::new(ACCOUNT_ID).unwrap(),
        ChannelId::new(CHANNEL_ID).unwrap(),
        "测试账号",
        Timestamp::now_utc(),
    )
}

/// 启用编排的处理器（与 `orchestration_store` 装配方向一致）。
fn enabled_handler(store: &Arc<SqliteStore>) -> OrcCommandHandler {
    let workflow = Workflow::preset(false).expect("预置工作流必须有效");
    OrcCommandHandler::new(Some(OrcStore::with_repository(workflow, store.clone())))
}

/// 装配捕获渠道 + 路由器（已 upsert 账号）；返回（捕获记录, 任务创建用的处理器, 路由器）。
fn router_with(
    store: &Arc<SqliteStore>,
    handler: OrcCommandHandler,
) -> (Arc<Mutex<Vec<OutboundMessage>>>, WechatOrcRouter) {
    let (channel, sent) = CaptureChannel::new();
    let mut registry = ChannelRegistry::default();
    registry.register(channel).unwrap();

    let router = WechatOrcRouter::new(
        handler,
        Arc::new(registry),
        store.clone(),
        Some(store.clone()),
    );
    (sent, router)
}

fn inbound(text: &str) -> InboundMessage {
    InboundMessage::new(
        InboundMessageId::new(format!("inbound-{}", text.len())).expect("入站 ID 必须有效"),
        ChannelId::new(CHANNEL_ID).unwrap(),
        ChannelAccountId::new(ACCOUNT_ID).unwrap(),
        InboundMessageInput {
            external_message_id: ExternalMessageId::new("ext-1").unwrap(),
            sender_id: "user-1".into(),
            conversation_id: CONVERSATION_ID.into(),
            referenced_message_ids: Vec::new(),
            text: text.into(),
            received_at: Timestamp::now_utc(),
        },
    )
    .expect("入站消息必须可构造")
}

/// 测试用工作目录：testkit 隔离根（必须已存在，创建任务时校验）。
fn existing_dir() -> String {
    agentnotify_testkit::test_temp_root()
        .to_string_lossy()
        .into_owned()
}

async fn create_task(creator: &OrcCommandHandler) -> String {
    let created = creator
        .create(CreateOrcTaskPayload {
            name: None,
            steps: None,
            goal: "做一个贪吃蛇游戏".into(),
            template_id: "preset-requirement-to-report".into(),
            working_dir: existing_dir(),
            notify_mode: None,
        })
        .await
        .expect("创建任务必须成功");
    // 本文件验证微信指令路由：任务先经「开始执行」（模拟用户在 UI 确认）再接受指令。
    creator
        .start(OrcTaskIdPayload {
            task_id: created.id.clone(),
        })
        .await
        .expect("开始执行必须成功");
    created.id
}

fn sent_texts(sent: &Arc<Mutex<Vec<OutboundMessage>>>) -> Vec<String> {
    sent.lock()
        .expect("捕获锁不应失败")
        .iter()
        .map(|message| message.text.clone())
        .collect()
}

/// 非集群消息（含 `【task ` 前缀与普通文本）必须放行走老路径，不发任何回执。
#[tokio::test]
async fn non_cluster_messages_pass_through_legacy_path() {
    let (_root, store) = open_sqlite("agentnotify-orc-wechat-pass-");
    store.upsert(test_account()).await.expect("账号必须可保存");
    let (sent, router) = router_with(&store, enabled_handler(&store));

    for text in ["你好", "【task task_9】确认", "【集群 task_9"] {
        let handled = router
            .intercept(&inbound(text))
            .await
            .expect("拦截器不得因非集群消息出错");
        assert!(!handled, "文本 {text} 必须走老路径");
    }
    assert!(sent.lock().unwrap().is_empty(), "非集群消息不得产生回执");
}

/// 未知正文默认按「指令」处理并回执任务摘要（Step/状态中文）。
#[tokio::test]
async fn cluster_instruction_defaults_and_replies() {
    let (_root, store) = open_sqlite("agentnotify-orc-wechat-instr-");
    store.upsert(test_account()).await.expect("账号必须可保存");
    let creator = enabled_handler(&store);
    let task_id = create_task(&creator).await;
    let (sent, router) = router_with(&store, enabled_handler(&store));

    let handled = router
        .intercept(&inbound(&format!("【集群 {task_id}】把重试加上")))
        .await
        .expect("拦截器必须成功");
    assert!(handled, "集群指令必须被消费");

    let reply = sent_texts(&sent);
    assert_eq!(reply.len(), 1, "恰好一条回执");
    assert!(reply[0].contains("Step 1"), "回执应含步骤：{}", reply[0]);
    assert!(
        reply[0].contains("干活中"),
        "回执应含中文状态：{}",
        reply[0]
    );
    assert!(
        reply[0].contains("指令已下发"),
        "回执应含动作说明：{}",
        reply[0]
    );

    let tasks = creator.list().await.expect("列出任务必须成功");
    assert_eq!(tasks[0].state, OrcTaskStateDto::Working);
    assert_eq!(tasks[0].current_step, 1, "指令不推进步骤，回到本步干活");
}

/// 回执线程上下文：purpose=Reply、会话与引用消息一致，用户可在微信线程内看到回执。
#[tokio::test]
async fn success_reply_carries_thread_context() {
    let (_root, store) = open_sqlite("agentnotify-orc-wechat-ctx-");
    store.upsert(test_account()).await.expect("账号必须可保存");
    let creator = enabled_handler(&store);
    let task_id = create_task(&creator).await;
    let (sent, router) = router_with(&store, enabled_handler(&store));

    router
        .intercept(&inbound(&format!("【集群 {task_id}】下一步")))
        .await
        .expect("拦截器必须成功");

    let messages = sent.lock().unwrap();
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].purpose, MessagePurpose::Reply);
    assert_eq!(messages[0].conversation_id, CONVERSATION_ID);
    assert_eq!(
        messages[0].reply_to.as_ref().map(ExternalMessageId::as_str),
        Some("ext-1"),
        "回执必须引用用户的指令消息"
    );
    assert!(!messages[0].client_id.is_empty(), "回执 client_id 不能为空");
}

/// 恢复只对 blocked 任务有效；成功后清阻塞、回到干活状态并回执。
#[tokio::test]
async fn recover_resumes_blocked_task_and_replies() {
    let (_root, store) = open_sqlite("agentnotify-orc-wechat-recover-");
    store.upsert(test_account()).await.expect("账号必须可保存");
    let creator = enabled_handler(&store);
    let task_id = create_task(&creator).await;
    creator
        .mark_blocked(MarkBlockedOrcTaskPayload {
            task_id: task_id.clone(),
            step: 1,
            reason: "opencode 会话不可用（未登录），消息未送达".into(),
        })
        .await
        .expect("标记阻塞必须成功");
    let (sent, router) = router_with(&store, enabled_handler(&store));

    let handled = router
        .intercept(&inbound(&format!("【集群 {task_id}】恢复")))
        .await
        .expect("拦截器必须成功");
    assert!(handled);

    let reply = sent_texts(&sent);
    assert_eq!(reply.len(), 1);
    assert!(reply[0].contains("恢复"), "回执应写清恢复：{}", reply[0]);

    let tasks = creator.list().await.expect("列出任务必须成功");
    assert_eq!(tasks[0].state, OrcTaskStateDto::Working, "恢复后回到干活中");
    assert_eq!(tasks[0].blocked_step, None, "阻塞标记必须清除");
}

/// 非阻塞任务发「恢复」必须明确报错（复用 task_not_blocked 语义）。
#[tokio::test]
async fn recover_on_unblocked_task_replies_clear_error() {
    let (_root, store) = open_sqlite("agentnotify-orc-wechat-recover2-");
    store.upsert(test_account()).await.expect("账号必须可保存");
    let creator = enabled_handler(&store);
    let task_id = create_task(&creator).await;
    let (sent, router) = router_with(&store, enabled_handler(&store));

    router
        .intercept(&inbound(&format!("【集群 {task_id}】恢复")))
        .await
        .expect("拦截器必须成功");

    let reply = sent_texts(&sent);
    assert_eq!(reply.len(), 1);
    assert!(
        reply[0].contains("未处于阻塞状态"),
        "非阻塞任务必须明确报错：{}",
        reply[0]
    );
}

/// 尚未汇报就发「确认」必须明确报错（confirm_before_report），不能静默吞掉。
#[tokio::test]
async fn confirm_without_report_replies_clear_error() {
    let (_root, store) = open_sqlite("agentnotify-orc-wechat-confirm-");
    store.upsert(test_account()).await.expect("账号必须可保存");
    let creator = enabled_handler(&store);
    let task_id = create_task(&creator).await;
    let (sent, router) = router_with(&store, enabled_handler(&store));

    router
        .intercept(&inbound(&format!("【集群 {task_id}】确认")))
        .await
        .expect("拦截器必须成功");

    let reply = sent_texts(&sent);
    assert_eq!(reply.len(), 1);
    assert!(
        reply[0].contains("确认无效"),
        "未收到汇报的确认必须明确拒绝：{}",
        reply[0]
    );
}

/// 任务不存在必须回执中文原因，不猜测兜底。
#[tokio::test]
async fn unknown_task_replies_not_found() {
    let (_root, store) = open_sqlite("agentnotify-orc-wechat-unknown-");
    store.upsert(test_account()).await.expect("账号必须可保存");
    let (sent, router) = router_with(&store, enabled_handler(&store));

    router
        .intercept(&inbound("【集群 no-such-task】指令"))
        .await
        .expect("拦截器必须成功");

    let reply = sent_texts(&sent);
    assert_eq!(reply.len(), 1);
    assert!(
        reply[0].contains("找不到任务"),
        "未知任务必须明确报错：{}",
        reply[0]
    );
}

/// 任务名寻址（推送头展示的形态）：按任务名也能下指令（旧的任务 ID 寻址保持兼容）。
#[tokio::test]
async fn instruction_by_task_name_resolves_and_replies() {
    let (_root, store) = open_sqlite("agentnotify-orc-wechat-name-");
    store.upsert(test_account()).await.expect("账号必须可保存");
    let creator = enabled_handler(&store);
    let _task_id = create_task(&creator).await;
    let (sent, router) = router_with(&store, enabled_handler(&store));

    // 任务未显式命名 → 展示名 = 目标前 8 字（与推送头一致）。
    router
        .intercept(&inbound("【集群 做一个贪吃蛇游戏】把重试加上"))
        .await
        .expect("拦截器必须成功");

    let reply = sent_texts(&sent);
    assert_eq!(reply.len(), 1);
    assert!(
        reply[0].contains("【做一个贪吃蛇游戏 · Step 1 · 干活中】"),
        "按任务名寻址必须命中并回执任务名：{}",
        reply[0]
    );
    assert!(reply[0].contains("指令已下发"), "{}", reply[0]);
}

/// 多个同名任务：不猜，明确要求消歧（用任务 ID 或到桌面端操作）。
#[tokio::test]
async fn ambiguous_task_name_replies_and_requires_disambiguation() {
    let (_root, store) = open_sqlite("agentnotify-orc-wechat-ambiguous-");
    store.upsert(test_account()).await.expect("账号必须可保存");
    let creator = enabled_handler(&store);
    create_task(&creator).await;
    create_task(&creator).await; // 同名第二个：目标相同 → 展示名相同
    let (sent, router) = router_with(&store, enabled_handler(&store));

    router
        .intercept(&inbound("【集群 做一个贪吃蛇游戏】指令"))
        .await
        .expect("拦截器必须成功");

    let reply = sent_texts(&sent);
    assert_eq!(reply.len(), 1);
    assert!(
        reply[0].contains("多个任务都叫"),
        "同名任务必须要求消歧：{}",
        reply[0]
    );
}

/// 空正文必须明确报错并给出可用格式提示。
#[tokio::test]
async fn empty_body_replies_format_error() {
    let (_root, store) = open_sqlite("agentnotify-orc-wechat-empty-");
    store.upsert(test_account()).await.expect("账号必须可保存");
    let creator = enabled_handler(&store);
    let task_id = create_task(&creator).await;
    let (sent, router) = router_with(&store, enabled_handler(&store));

    router
        .intercept(&inbound(&format!("【集群 {task_id}】")))
        .await
        .expect("拦截器必须成功");

    let reply = sent_texts(&sent);
    assert_eq!(reply.len(), 1);
    assert!(
        reply[0].contains("缺少正文"),
        "空正文必须明确报错：{}",
        reply[0]
    );
}

/// 编排未启用（handler 未装配仓储）时回执「编排未启用」，消息仍被消费（不落引用回复路由）。
#[tokio::test]
async fn disabled_orchestration_replies_enable_hint() {
    let (_root, store) = open_sqlite("agentnotify-orc-wechat-disabled-");
    store.upsert(test_account()).await.expect("账号必须可保存");
    let (sent, router) = router_with(&store, OrcCommandHandler::new(None));

    let handled = router
        .intercept(&inbound("【集群 whatever】指令"))
        .await
        .expect("拦截器必须成功");
    assert!(handled, "识别到的指令必须被消费，不落引用回复路由");

    let reply = sent_texts(&sent);
    assert_eq!(reply.len(), 1);
    assert!(
        reply[0].contains("编排未启用"),
        "未启用编排必须回执中文原因：{}",
        reply[0]
    );
}

/// 回执对象（账号）缺失时：消费消息但不执行指令（避免执行成功却无回执、用户重复发送）。
#[tokio::test]
async fn missing_account_consumes_without_executing() {
    // 不 upsert 账号：模拟账号不存在（在正常链路中不可达，这里钉住防御行为）。
    let (_root, store) = open_sqlite("agentnotify-orc-wechat-noacct-");
    let creator = enabled_handler(&store);
    let task_id = create_task(&creator).await;
    let (sent, router) = router_with(&store, enabled_handler(&store));

    let handled = router
        .intercept(&inbound(&format!("【集群 {task_id}】下一步")))
        .await
        .expect("拦截器必须成功");
    assert!(handled, "必须按已消费处理，防止消息落到引用回复路由");
    assert!(
        sent.lock().unwrap().is_empty(),
        "无法回执时不得产生半截回执"
    );

    let tasks = creator.list().await.expect("列出任务必须成功");
    assert_eq!(tasks[0].current_step, 1, "无法回执时不得执行指令");
}

/// 集群指令推进经 advance（指令）→ mark_blocked → 恢复 的完整闭环仍可用（回归既有命令能力）。
#[tokio::test]
async fn wechat_commands_reuse_orc_command_handler_capabilities() {
    let (_root, store) = open_sqlite("agentnotify-orc-wechat-chain-");
    store.upsert(test_account()).await.expect("账号必须可保存");
    let creator = enabled_handler(&store);
    let task_id = create_task(&creator).await;
    let (sent, router) = router_with(&store, enabled_handler(&store));

    // 指令：回到本步干活（Step 1 · 干活中）
    router
        .intercept(&inbound(&format!("【集群 {task_id}】指令")))
        .await
        .expect("拦截器必须成功");

    // 桌面侧汇报推进到第 2 步（复用 bridge 命令能力）
    creator
        .advance(AdvanceOrcTaskPayload {
            task_id: task_id.clone(),
            kind: OrcMessageKindDto::Report,
        })
        .await
        .expect("汇报推进必须成功");

    // 微信恢复
    let blocked = creator
        .mark_blocked(MarkBlockedOrcTaskPayload {
            task_id: task_id.clone(),
            step: 2,
            reason: "opencode 未登录".into(),
        })
        .await
        .expect("标记阻塞必须成功");
    assert_eq!(blocked.state, OrcTaskStateDto::Failed);

    router
        .intercept(&inbound(&format!("【集群 {task_id}】重发")))
        .await
        .expect("拦截器必须成功");

    let tasks = creator.list().await.expect("列出任务必须成功");
    assert_eq!(tasks[0].state, OrcTaskStateDto::Working);
    assert_eq!(tasks[0].current_step, 2, "恢复后回到原步骤继续干活");
    assert_eq!(tasks[0].blocked_step, None);

    let replies = sent_texts(&sent);
    assert_eq!(replies.len(), 2, "两次指令各一条回执");
    let resumed = &replies[1];
    assert!(
        resumed.contains("恢复") && resumed.contains("Step 2"),
        "{resumed}"
    );
}

/// 错误回执保持 Reply 语义与线程上下文（与成功回执一致，用户在同一线程看到失败原因）。
#[tokio::test]
async fn error_reply_carries_thread_context() {
    let (_root, store) = open_sqlite("agentnotify-orc-wechat-errctx-");
    store.upsert(test_account()).await.expect("账号必须可保存");
    let (sent, router) = router_with(&store, enabled_handler(&store));

    router
        .intercept(&inbound("【集群 no-such-task】确认"))
        .await
        .expect("拦截器必须成功");

    let messages = sent.lock().unwrap();
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].purpose, MessagePurpose::Reply);
    assert_eq!(messages[0].conversation_id, CONVERSATION_ID);
    assert_eq!(
        messages[0].reply_to.as_ref().map(ExternalMessageId::as_str),
        Some("ext-1")
    );
}
