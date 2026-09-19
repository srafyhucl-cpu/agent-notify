use agentnotify_channel_sdk::ChannelError;
use agentnotify_domain::Timestamp;

use crate::descriptor::MAX_TEXT_BYTES;

const SUCCESS_BADGE: &str = "🟢";
const TITLE_BAR: &str = "｜";
const DIVIDER: &str = "—";
const REPLY_HINT: &str = "*引用此消息可继续对话*";

/// 由 Runtime 从 Agent Registry descriptor 和 Notification 组装的标准通知内容。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NotificationRenderInput {
    pub agent_display_name: String,
    pub session_name: String,
    pub body: String,
    pub occurred_at: Timestamp,
    pub include_footer: bool,
    pub replyable: bool,
}

/// 渲染 ClawBot Markdown；超过渠道字节上限时明确拒绝，不静默截断。
pub fn render_notification(input: NotificationRenderInput) -> Result<String, ChannelError> {
    let agent_display_name = input.agent_display_name.trim();
    if agent_display_name.is_empty() {
        return Err(ChannelError::permanent(
            "clawbot_render_agent_empty",
            "通知缺少 Agent 显示名称",
        ));
    }
    let session_name = input.session_name.trim();
    if session_name.is_empty() {
        return Err(ChannelError::permanent(
            "clawbot_render_session_empty",
            "通知缺少会话名称",
        ));
    }
    if input.body.trim().is_empty() {
        return Err(ChannelError::permanent(
            "clawbot_render_body_empty",
            "通知正文不能为空",
        ));
    }

    let mut rendered = format!(
        "**{SUCCESS_BADGE} {agent_display_name}{TITLE_BAR}{session_name}**\n\n{}",
        input.body
    );
    if input.include_footer {
        rendered.push_str("\n\n");
        rendered.push_str(DIVIDER);
        rendered.push('\n');
        if input.replyable {
            rendered.push_str(REPLY_HINT);
            rendered.push_str(" · ");
        }
        rendered.push_str(&format_time(input.occurred_at)?);
    }

    if rendered.len() > MAX_TEXT_BYTES {
        return Err(ChannelError::permanent(
            "clawbot_text_too_large",
            "ClawBot 通知超过 32 KiB 上限",
        ));
    }
    Ok(rendered)
}

fn format_time(timestamp: Timestamp) -> Result<String, ChannelError> {
    let timestamp = time::OffsetDateTime::parse(
        &timestamp.to_rfc3339(),
        &time::format_description::well_known::Rfc3339,
    )
    .map_err(|_| ChannelError::permanent("clawbot_render_time_invalid", "通知时间格式无效"))?;
    Ok(format!(
        "{:02}/{:02} {:02}:{:02}",
        u8::from(timestamp.month()),
        timestamp.day(),
        timestamp.hour(),
        timestamp.minute()
    ))
}
