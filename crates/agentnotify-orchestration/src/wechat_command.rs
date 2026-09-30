//! 微信集群指令识别（§9 D-入口「微信回复」侧，P1-3）。
//!
//! 识别规则（定死方案，避免过度设计）：
//! - 只有以「【集群 」开头、随后是「任务地址」（任务名或任务 ID）、以「】」收尾的消息才判定为集群指令；
//! - **不兼容「【task 」前缀**：与既有普通任务消息/引用回复区分开，避免误判；
//! - 其余任何消息都不是集群指令（`parse_cluster_command` 返回 `None`），完全走既有回复链路。
//!
//! 指令正文映射（收窄为 3 个动作，`report/question/info` 不向用户暴露）：
//!
//! | 正文开头 | 动作 | 落点 |
//! |---|---|---|
//! | 确认 / 完成 / confirm | `Confirm` | `advance(confirm)`，通过人工确认门 |
//! | 恢复 / recover / 重发 | `Recover` | `recover_blocked`，仅对 blocked 任务有效 |
//! | 指令 / 下一步 / instruction | `Instruction` | `advance(instruction)`，回到本步/重新发起 |
//! | 其他正文 | `Instruction`（默认兜底，不丢指令） | 同上 |
//!
//! 语义约定：`confirm` 只有任务处于「等待确认」时才有效（状态机负责拒绝）；`恢复` 只对
//! blocked（A2A `failed`）任务有效，非阻塞任务由 `recover_blocked` 明确报错。

/// 集群指令固定前缀（设计文档 §4.6/S9：`【集群 <任务名或任务ID>】`）。
pub const WECHAT_CLUSTER_PREFIX: &str = "【集群 ";

/// 识别出的集群指令：任务地址（任务名或任务 ID）+ 指令正文（可能为空，由路由层报错，不猜测兜底）。
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct ClusterCommand {
    /// 任务地址：任务名（面向用户）或任务 ID（兼容旧消息）；由路由层解析成任务 ID。
    pub address: String,
    pub body: String,
}

/// 微信指令正文映射出的三个安全动作（`report/question/info` 不向用户暴露）。
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum WechatAction {
    /// 确认：通过人工确认门（`advance(Confirm)`）
    Confirm,
    /// 指令：回到本步干活 / 从 blocked 重新发起（`advance(Instruction)`）
    Instruction,
    /// 恢复：仅对 blocked 任务有效（`recover_blocked`）
    Recover,
}

impl WechatAction {
    /// 稳定的中文动作名（用户回执与日志用）。
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Confirm => "确认",
            Self::Instruction => "指令",
            Self::Recover => "恢复",
        }
    }
}

/// 识别集群指令：`【集群 <任务名或任务ID>】正文`。格式不符返回 `None`（走老路径）。
///
/// 正文允许为空（返回 `Some` 且 `body` 为空），由路由层给用户明确报错；
/// 地址为空（如 `【集群 】确认`）视为格式不符。
///
/// 寻址以**第一个 `】`** 收尾：任务名禁止包含 `】`/`【`（创建/编辑侧校验），
/// 否则地址会被截断（B3）。
pub fn parse_cluster_command(text: &str) -> Option<ClusterCommand> {
    let text = text.trim();
    let rest = text.strip_prefix(WECHAT_CLUSTER_PREFIX)?;
    let close = rest.find('】')?;
    let address = rest[..close].trim();
    if address.is_empty() {
        return None;
    }
    let body = rest[close + '】'.len_utf8()..].trim();
    Some(ClusterCommand {
        address: address.to_owned(),
        body: body.to_owned(),
    })
}

/// 指令正文 → 动作。顺序：确认 → 恢复 → 指令；都不匹配时默认按「指令」处理（兜底不丢指令）。
pub fn parse_wechat_action(body: &str) -> WechatAction {
    let body = body.trim();
    let lower = body.to_ascii_lowercase();
    if keywords_match(&lower, body, CONFIRM_KEYWORDS) {
        return WechatAction::Confirm;
    }
    if keywords_match(&lower, body, RECOVER_KEYWORDS) {
        return WechatAction::Recover;
    }
    if keywords_match(&lower, body, INSTRUCTION_KEYWORDS) {
        return WechatAction::Instruction;
    }
    WechatAction::Instruction
}

/// 确认动作关键词（`confirm` 按英文单词匹配，忽略大小写）。
const CONFIRM_KEYWORDS: [&str; 3] = ["确认", "完成", "confirm"];
/// 恢复动作关键词；仅对 blocked 任务有效。
const RECOVER_KEYWORDS: [&str; 3] = ["恢复", "重发", "recover"];
/// 指令动作关键词。
const INSTRUCTION_KEYWORDS: [&str; 3] = ["指令", "下一步", "instruction"];

/// 关键词匹配：中文按「正文开头」匹配（微信无空格分词）；英文按「开头 + 词边界」匹配
/// （忽略大小写），避免 `confirming` 之类误判，同时容忍 `confirm，继续` 这类中英混排。
fn keywords_match(lower: &str, original: &str, keywords: [&str; 3]) -> bool {
    keywords.iter().any(|keyword| {
        if keyword.chars().all(|c| c.is_ascii_alphabetic()) {
            lower.strip_prefix(keyword).is_some_and(|rest| {
                rest.is_empty()
                    || !rest
                        .chars()
                        .next()
                        .is_some_and(|c| c.is_ascii_alphanumeric())
            })
        } else {
            original.starts_with(keyword)
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 识别：标准格式 + 空白容忍 + 空正文（交给路由层报错）。
    #[test]
    fn parse_cluster_command_extracts_task_id_and_body() {
        let command = parse_cluster_command("【集群 task_9】确认").expect("标准格式必须识别");
        assert_eq!(command.address, "task_9");
        assert_eq!(command.body, "确认");

        let command =
            parse_cluster_command("  【集群 task_9】  确认，可以  ").expect("首尾空白必须容忍");
        assert_eq!(command.address, "task_9");
        assert_eq!(command.body, "确认，可以");

        let command = parse_cluster_command("【集群 task_9】").expect("无正文也要识别出 task_id");
        assert_eq!(command.address, "task_9");
        assert!(command.body.is_empty(), "空正文保持为空，由路由层明确报错");
    }

    /// 定死方案：`【task ` 前缀不识别（避免与普通任务消息混淆），完全走老路径。
    #[test]
    fn task_prefix_is_not_a_cluster_command() {
        assert_eq!(parse_cluster_command("【task task_9】确认"), None);
        assert_eq!(
            parse_cluster_command("【集群 task_9"),
            None,
            "缺少收尾】不是集群指令"
        );
        assert_eq!(
            parse_cluster_command("【集群 】确认"),
            None,
            "空 task_id 不是集群指令"
        );
        assert_eq!(parse_cluster_command("【集群  】"), None);
        assert_eq!(parse_cluster_command("普通消息"), None);
        assert_eq!(parse_cluster_command(""), None);
    }

    /// B3：正常任务名解析不受影响；地址段内含/后接 `】` 时只在第一个 `】` 收尾，
    /// 不会误定位到后面的文字（这正是创建/编辑禁止 `】`/`【` 的原因）。
    #[test]
    fn address_separator_does_not_misroute() {
        // 正常任务名：解析不受影响。
        let command = parse_cluster_command("【集群 发版自检-闭环】确认").expect("正常名必须解析");
        assert_eq!(command.address, "发版自检-闭环");
        assert_eq!(command.body, "确认");

        // 名称里混入分隔符（创建侧已拒绝）：地址在第一个 `】` 截断，不会跳到后续候选名。
        let command = parse_cluster_command("【集群 发版自检】-闭环】确认").expect("仍能解析");
        assert_eq!(command.address, "发版自检", "地址在第一个 】 截断");
        assert_eq!(command.body, "-闭环】确认");

        // 空地址后接 `】`：仍是无效指令，不会落到后面的「甲」。
        assert_eq!(parse_cluster_command("【集群 】甲】确认"), None);
    }

    /// 动作映射表：确认 / 完成 / 恢复 / 重发 / 指令 / 下一步 + 英文（忽略大小写、按词匹配）。
    #[test]
    fn action_mapping_table() {
        for (text, expected) in [
            ("确认", WechatAction::Confirm),
            ("完成", WechatAction::Confirm),
        ] {
            assert_eq!(parse_wechat_action(text), expected, "正文 {text}");
        }
        assert_eq!(parse_wechat_action("confirm"), WechatAction::Confirm);
        assert_eq!(
            parse_wechat_action("CONFIRM，继续"),
            WechatAction::Confirm,
            "英文忽略大小写、容忍中文符号拼接"
        );
        assert_eq!(
            parse_wechat_action("confirmation"),
            WechatAction::Instruction,
            "confirming/confirmation 不算确认"
        );

        for (text, expected) in [
            ("恢复", WechatAction::Recover),
            ("重发", WechatAction::Recover),
        ] {
            assert_eq!(parse_wechat_action(text), expected, "正文 {text}");
        }
        assert_eq!(parse_wechat_action("recover"), WechatAction::Recover);
        assert_eq!(
            parse_wechat_action("Recover please"),
            WechatAction::Recover,
            "英文按单词匹配"
        );

        for (text, expected) in [
            ("指令", WechatAction::Instruction),
            ("下一步", WechatAction::Instruction),
        ] {
            assert_eq!(parse_wechat_action(text), expected, "正文 {text}");
        }
        assert_eq!(
            parse_wechat_action("instruction"),
            WechatAction::Instruction
        );

        // 默认兜底：未知正文一律按「指令」处理，不丢指令
        assert_eq!(parse_wechat_action("把重试加上"), WechatAction::Instruction);
        assert_eq!(parse_wechat_action("继续执行"), WechatAction::Instruction);
        assert_eq!(parse_wechat_action(""), WechatAction::Instruction);
    }

    /// 中文开头匹配的边界：整词开头才命中，避免把「汇报」当「恢复」。
    #[test]
    fn chinese_keywords_match_by_leading_text() {
        assert_eq!(parse_wechat_action("恢复执行"), WechatAction::Recover);
        assert_eq!(parse_wechat_action("确认可以继续"), WechatAction::Confirm);
        assert_eq!(
            parse_wechat_action("下一步做什么"),
            WechatAction::Instruction
        );
        assert_eq!(
            parse_wechat_action("明天恢复"),
            WechatAction::Instruction,
            "「恢复」不在开头时不命中"
        );
    }
}
