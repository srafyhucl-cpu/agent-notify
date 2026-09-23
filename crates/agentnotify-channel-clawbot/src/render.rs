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
    /// 页脚时间戳使用的 UTC 偏移（分钟）；None 表示用系统本地时区。
    /// 生产路径传 None（与 Go 版一致：显示用户本地时间）；测试传固定偏移以保证跨机器确定性。
    pub utc_offset_minutes: Option<i16>,
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
        rendered.push_str(&format_time(input.occurred_at, input.utc_offset_minutes)?);
    }

    if rendered.len() > MAX_TEXT_BYTES {
        return Err(ChannelError::permanent(
            "clawbot_text_too_large",
            "ClawBot 通知超过 32 KiB 上限",
        ));
    }
    Ok(rendered)
}

/// 渲染页脚时间。`offset_minutes` 为 None 时用系统本地时区（Go 版行为：显示用户本地时间），
/// 时间为 UTC（`now_utc`）时必须偏移，否则页脚会比用户本地时间早/晚数小时。
fn format_time(timestamp: Timestamp, offset_minutes: Option<i16>) -> Result<String, ChannelError> {
    let parsed = time::OffsetDateTime::parse(
        &timestamp.to_rfc3339(),
        &time::format_description::well_known::Rfc3339,
    )
    .map_err(|_| ChannelError::permanent("clawbot_render_time_invalid", "通知时间格式无效"))?;
    let offset = match offset_minutes {
        Some(minutes) => {
            time::UtcOffset::from_whole_seconds(i32::from(minutes) * 60).map_err(|_| {
                ChannelError::permanent("clawbot_render_time_offset_invalid", "通知时间偏移无效")
            })?
        }
        None => time::UtcOffset::current_local_offset().map_err(|_| {
            ChannelError::permanent(
                "clawbot_render_time_offset_unavailable",
                "无法获取系统本地时区偏移，通知页脚时间无法渲染",
            )
        })?,
    };
    let local = parsed.to_offset(offset);
    Ok(format!(
        "{:02}/{:02} {:02}:{:02}",
        u8::from(local.month()),
        local.day(),
        local.hour(),
        local.minute()
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn timestamp(value: &str) -> Timestamp {
        Timestamp::parse_rfc3339(value).unwrap()
    }

    /// 页脚时间必须按给定偏移渲染：UTC 06:34 在 UTC+8 应显示 14:34（真机缺陷回归）。
    #[test]
    fn format_time_uses_given_offset() {
        let instant = timestamp("2026-09-23T06:34:00Z");
        assert_eq!(format_time(instant, Some(8 * 60)).unwrap(), "09/23 14:34");
        assert_eq!(format_time(instant, Some(0)).unwrap(), "09/23 06:34");
        assert_eq!(format_time(instant, Some(-5 * 60)).unwrap(), "09/23 01:34");
    }

    /// 不给偏移时用系统本地时区；平台不支持获取本地偏移时跳过（Windows 必然支持）。
    #[test]
    fn format_time_defaults_to_system_local_offset() {
        let instant = timestamp("2026-09-23T06:34:00Z");
        let Ok(offset) = time::UtcOffset::current_local_offset() else {
            return;
        };
        let minutes = offset.whole_minutes();
        assert_eq!(
            format_time(instant, None).unwrap(),
            format_time(instant, Some(minutes)).unwrap()
        );
    }
}
