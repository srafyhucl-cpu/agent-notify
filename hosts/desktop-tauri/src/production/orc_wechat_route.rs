//! 微信集群指令路由（P1-3，§9 D-入口「微信回复」侧）。
//!
//! 实现 [`InboundInterceptor`]：把「【集群 <task_id>】…」的入站消息识别为集群指令，
//! **复用 [`OrcCommandHandler`] 的 advance / recover_blocked 业务能力**推进任务，
//! 并向微信回执结果（成功摘要或明确的中文失败原因，不静默吞掉）。
//!
//! 识别与映射规则（定死方案，解析纯函数在 `agentnotify-orchestration::wechat_command`）：
//! - 前缀「【集群 」+ task_id + 「】」；不兼容「【task 」前缀，格式不符完全走既有引用回复链路；
//! - 确认 / 完成 / confirm → `Confirm`（advance）；恢复 / recover / 重发 → `Recover`
//!   （recover_blocked，仅对 blocked 任务有效）；指令 / 下一步 / instruction → `Instruction`
//!   （advance）；其余正文默认按「指令」处理（兜底不丢指令）；
//! - `report / question / info` 不向用户暴露（收窄为 confirm / instruction / recover 三个动作）。
//!
//! 零影响承诺：非「【集群 」开头的消息返回 `Ok(false)`，完全走老路径；
//! 编排未启用（`orchestration.enabled=false`）时识别到的指令回执「编排未启用」中文原因。

use std::sync::Arc;

use agentnotify_application::{ChannelAccountStore, StatusStore};
use agentnotify_channel_sdk::{
    ChannelAccount, ChannelAdapter, ChannelError, ChannelRegistry, DeliveryReceipt, MessagePurpose,
    OutboundMessage,
};
use agentnotify_domain::{InboundMessage, SafeError};
use agentnotify_orchestration::{
    ClusterCommand, WechatAction, parse_cluster_command, parse_wechat_action,
};
use agentnotify_runtime::{InboundInterceptor, RuntimeTargetError};
use agentnotify_storage_sqlite::SqliteStore;

use super::service::OrcCommandHandler;
use crate::bridge::dto::{
    AdvanceOrcTaskPayload, OrcMessageKindDto, OrcTaskDto, OrcTaskIdPayload, OrcTaskStateDto,
};
use crate::bridge::error::CommandError;

/// 识别到集群指令但渠道不可用导致无法回执时的稳定错误码（日志用）。
const ORC_WECHAT_ROUTER_UNREACHABLE: &str = "orc_wechat_router_unavailable";
/// 渠道未确认集群指令回执（投递层失败，不影响任务状态）。
const ORC_WECHAT_REPLY_UNCONFIRMED: &str = "orc_wechat_reply_unconfirmed";
/// 编排未启用时由 `OrcCommandHandler` 返回的稳定错误码（service.rs 常量）。
const ORCHESTRATION_DISABLED_CODE: &str = "orchestration_disabled";
/// 指令正文为空时的错误码（识别成功但内容缺失，明确报错不猜测）。
const ORC_WECHAT_BODY_EMPTY: &str = "orc_wechat_body_empty";

/// 微信集群指令路由器：识别「【集群 <task_id>】…」消息并调用编排命令，面向用户回执。
pub struct WechatOrcRouter {
    handler: OrcCommandHandler,
    channels: Arc<ChannelRegistry>,
    accounts: Arc<SqliteStore>,
    status: Option<Arc<dyn StatusStore>>,
}

impl WechatOrcRouter {
    /// `handler` 复用 `OrcCommandHandler`（与 bridge 命令同一份业务实现）；
    /// `status` 用于记录回执投递失败（诊断可见，与 `ReplyService` 的提示失败记录一致）。
    pub fn new(
        handler: OrcCommandHandler,
        channels: Arc<ChannelRegistry>,
        accounts: Arc<SqliteStore>,
        status: Option<Arc<dyn StatusStore>>,
    ) -> Self {
        Self {
            handler,
            channels,
            accounts,
            status,
        }
    }

    /// 执行指令：正文为空明确报错；否则按映射调用 advance / recover_blocked。
    async fn route(&self, command: &ClusterCommand) -> Result<OrcTaskDto, CommandError> {
        if command.body.trim().is_empty() {
            return Err(CommandError::new(
                ORC_WECHAT_BODY_EMPTY,
                "集群指令缺少正文：请在【集群 <任务ID>】后附上指令（确认 / 指令内容 / 恢复）",
            ));
        }
        match parse_wechat_action(&command.body) {
            WechatAction::Confirm => {
                self.handler
                    .advance(AdvanceOrcTaskPayload {
                        task_id: command.task_id.clone(),
                        kind: OrcMessageKindDto::Confirm,
                    })
                    .await
            }
            WechatAction::Instruction => {
                self.handler
                    .advance(AdvanceOrcTaskPayload {
                        task_id: command.task_id.clone(),
                        kind: OrcMessageKindDto::Instruction,
                    })
                    .await
            }
            WechatAction::Recover => {
                self.handler
                    .recover_blocked(OrcTaskIdPayload {
                        task_id: command.task_id.clone(),
                    })
                    .await
            }
        }
    }

    /// 向绑定私聊回执结果；失败只记录（诊断可见），不重试、不影响任务状态。
    async fn reply(
        &self,
        message: &InboundMessage,
        channel: &dyn ChannelAdapter,
        account: ChannelAccount,
        text: String,
    ) {
        let capabilities = channel.capabilities();
        let outbound = OutboundMessage {
            purpose: MessagePurpose::Reply,
            conversation_id: message.conversation_id.clone(),
            text,
            client_id: format!("orc-wechat-{}", message.id.as_str()),
            reply_to: message.external_message_id.clone(),
            notification: None,
            safe_metadata: Default::default(),
        };
        match channel.send(account, outbound).await {
            Ok(receipt) if valid_notice_receipt(&receipt, &capabilities) => {}
            Ok(_) => {
                self.record_notice_error(
                    SafeError::new(ORC_WECHAT_REPLY_UNCONFIRMED, "渠道未确认集群指令回执")
                        .expect("内置安全错误必须有效"),
                )
                .await;
            }
            Err(error) => self.record_notice_error(channel_error_safe(error)).await,
        }
    }

    async fn record_notice_error(&self, error: SafeError) {
        tracing::warn!(code = error.code(), "集群指令回执发送失败，不影响任务状态");
        if let Some(status) = &self.status {
            if let Err(store_error) = status.record_error(error).await {
                tracing::warn!(code = store_error.code(), "记录集群指令回执错误失败");
            }
        }
    }
}

#[async_trait::async_trait]
impl InboundInterceptor for WechatOrcRouter {
    async fn intercept(&self, message: &InboundMessage) -> Result<bool, RuntimeTargetError> {
        let Some(command) = parse_cluster_command(&message.text) else {
            // 非「【集群 」格式：完全走既有引用回复链路，零影响。
            return Ok(false);
        };
        // 已识别为集群指令：本消息不再进入引用回复路由（识别即消费，与编排开关无关）。
        tracing::info!(task_id = %command.task_id, "识别到微信集群指令");

        let Some(channel) = self.channels.get(&message.channel_id) else {
            tracing::warn!(
                code = ORC_WECHAT_ROUTER_UNREACHABLE,
                "识别到集群指令但渠道适配器不可用，无法回执，消息已消费"
            );
            return Ok(true);
        };
        // 无法确认回执对象时不执行指令（避免执行成功但用户无感知、重复发送导致重复推进）。
        let account = match self.accounts.get(&message.account_id).await {
            Ok(Some(account)) => account,
            Ok(None) => {
                tracing::warn!(
                    code = ORC_WECHAT_ROUTER_UNREACHABLE,
                    "识别到集群指令但渠道账号不存在，消息已消费"
                );
                return Ok(true);
            }
            Err(error) => {
                tracing::warn!(
                    code = error.code(),
                    "识别到集群指令但账号查询失败，消息已消费"
                );
                return Ok(true);
            }
        };

        let action = parse_wechat_action(&command.body);
        let text = match self.route(&command).await {
            Ok(dto) => success_text(action, &dto),
            Err(error) => {
                if error.code() != ORCHESTRATION_DISABLED_CODE {
                    tracing::warn!(code = error.code(), task_id = %command.task_id, "集群指令执行失败");
                }
                format!("集群指令未生效：{}", error.message())
            }
        };
        self.reply(message, channel.as_ref(), account, text).await;
        Ok(true)
    }
}

/// 成功回执：[任务摘要] + 动作说明（状态用中文，用户直接可读）。
fn success_text(action: WechatAction, dto: &OrcTaskDto) -> String {
    let action_cn = match action {
        WechatAction::Confirm => "已确认，任务按指令推进",
        WechatAction::Instruction => "指令已下发，任务继续推进",
        WechatAction::Recover => "任务已恢复执行",
    };
    format!(
        "【{} · Step {} · {}】{}",
        dto.id,
        dto.current_step,
        state_cn(&dto.state),
        action_cn
    )
}

/// 任务状态 → 中文（微信回执用；未覆盖的新状态不猜测，落到稳定枚举名）。
fn state_cn(state: &OrcTaskStateDto) -> &'static str {
    match state {
        OrcTaskStateDto::Working => "干活中",
        OrcTaskStateDto::InputRequired => "等待确认",
        OrcTaskStateDto::Completed => "已完成",
        OrcTaskStateDto::Failed => "已阻塞",
        OrcTaskStateDto::Unspecified => "未指定",
        OrcTaskStateDto::Submitted => "已提交",
        OrcTaskStateDto::Canceled => "已取消",
        OrcTaskStateDto::Rejected => "已拒绝",
        OrcTaskStateDto::AuthRequired => "需认证",
    }
}

fn valid_notice_receipt(
    receipt: &DeliveryReceipt,
    capabilities: &agentnotify_channel_sdk::ChannelCapabilities,
) -> bool {
    receipt.state == agentnotify_domain::DeliveryState::Sent && receipt.is_valid_for(capabilities)
}

fn channel_error_safe(error: ChannelError) -> SafeError {
    SafeError::new(error.code(), error.message()).expect("渠道适配器错误常量必须有效")
}
