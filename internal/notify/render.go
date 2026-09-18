package notify

import (
	"fmt"
	"strings"
	"time"

	"github.com/srafyhucl-cpu/agent-notify/internal/agentmeta"
)

const (
	notificationSeparator = "\n\n"
	// notificationDivider 用全角破折号做页脚前的短分隔符：纯文本客户端里是一条干净的短线，
	// 不抢正文注意力。
	notificationDivider  = "—"
	notificationTitleBar = "｜"
	// replyHintText 既用于页脚提示，也用于清理 Agent 误带的旧页脚。
	replyHintText    = "引用此消息可继续对话"
	footerTimeLayout = "01/02 15:04"

	genericTitlePrefix  = "【通知】"
	genericDefaultTitle = "任务已完成"
	genericDisplayName  = "通知"
	defaultBody         = "任务已完成。"
)

type renderedNotification struct {
	Title   string
	Summary string
	Message string
	// SessionName 是清洗后的会话名，供引用回复的送达确认复用。
	SessionName string
}

func renderNotification(opts NotifyOptions, now time.Time) renderedNotification {
	sessionName := notificationSessionName(opts.Agent, opts.Title)
	title := markdownNotificationTitle(opts.Agent, sessionName, opts.Notice)
	summary := FormatNotifySummary(opts.Summary, 0)
	if summary == "" {
		summary = defaultBody
	}
	if notice := strings.TrimSpace(opts.Notice); notice != "" {
		formattedNotice := "> ⚠️ " + strings.ReplaceAll(notice, "\n", "\n> ")
		summary = strings.TrimSpace(summary + notificationSeparator + formattedNotice)
	}
	footer := notificationFooter(opts.Agent, now, opts.ReplyWindowSec)

	message := composeNotification(title, summary, footer)
	if opts.MaxChars > 0 {
		title, summary, message = fitNotification(title, summary, footer, opts.MaxChars)
	}
	return renderedNotification{Title: title, Summary: summary, Message: message, SessionName: sessionName}
}

// notificationSessionName 提取用于展示与引用确认的会话名：去掉旧前缀、状态徽标与加粗符号。
func notificationSessionName(agentName, rawTitle string) string {
	clean := strings.TrimSpace(strings.ReplaceAll(rawTitle, "\n", " "))
	clean = strings.Trim(clean, "*")
	clean = stripStatusBadge(clean)
	if descriptor, ok := agentmeta.Lookup(agentName); ok {
		// 剥离可能存在的各种旧前缀（大小写不限）。
		for _, prefix := range []string{
			descriptor.TitlePrefix,
			"【" + strings.ToLower(descriptor.ID) + "】",
			"【" + strings.ToUpper(descriptor.ID) + "】",
			"【" + descriptor.DisplayName + "】",
		} {
			clean = strings.TrimSpace(strings.TrimPrefix(clean, prefix))
		}
		if clean == "" || clean == "跑完了" || clean == "opencode会话" {
			clean = descriptor.DefaultTitle
		}
		return clean
	}

	clean = strings.TrimSpace(strings.TrimPrefix(clean, genericTitlePrefix))
	if clean == "" || clean == genericDefaultTitle || clean == "任务完成" || clean == "跑完了" {
		clean = genericDefaultTitle
	}
	return clean
}

// markdownNotificationTitle 生成加粗标题行：**🟢 Codex｜会话名**。
func markdownNotificationTitle(agentName, sessionName, notice string) string {
	badge := "🟢"
	if isFailureText(notice) || isFailureText(sessionName) {
		badge = "⚠️"
	}
	name := genericDisplayName
	if descriptor, ok := agentmeta.Lookup(agentName); ok {
		name = descriptor.DisplayName
	}
	return "**" + badge + " " + name + notificationTitleBar + sessionName + "**"
}

func isFailureText(value string) bool {
	return strings.Contains(value, "错误") || strings.Contains(value, "失败") || strings.Contains(value, "异常")
}

func stripStatusBadge(value string) string {
	return strings.TrimSpace(strings.TrimLeft(value, "🟢⚠️🔴⚡ "))
}

func notificationFooter(agentName string, now time.Time, replyWindowSec int) string {
	descriptor, ok := agentmeta.Lookup(agentName)
	if !ok {
		// 通用通知没有 Agent 归属，保持无页脚（与旧行为一致）。
		return ""
	}
	parts := make([]string, 0, 2)
	if descriptor.Replyable {
		hint := replyHintText
		if replyWindowSec > 0 {
			// 有等待窗口时写明时限，避免用户错过才来引用。
			hint = fmt.Sprintf("%s（%d 秒内）", replyHintText, replyWindowSec)
		}
		// 只给提示文字加斜体，时间保持正体。
		parts = append(parts, "*"+hint+"*")
	}
	parts = append(parts, now.Format(footerTimeLayout))
	return notificationDivider + "\n" + strings.Join(parts, " · ")
}

func composeNotification(title, summary, footer string) string {
	message := title
	if summary != "" {
		message += notificationSeparator + summary
	}
	if footer != "" {
		message += notificationSeparator + footer
	}
	return message
}

func fitNotification(title, summary, footer string, maxChars int) (string, string, string) {
	message := composeNotification(title, summary, footer)
	if maxChars <= 0 || runeCount(message) <= maxChars {
		return title, summary, message
	}

	overhead := runeCount(title) + runeCount(footer)
	if summary != "" {
		overhead += 2 * runeCount(notificationSeparator)
	}
	available := maxChars - overhead
	if available > 0 {
		summary = CutSentence(summary, available)
		message = composeNotification(title, summary, footer)
		if runeCount(message) <= maxChars {
			return title, summary, message
		}
	}

	message = composeNotification(title, "", footer)
	if runeCount(message) <= maxChars {
		return title, "", message
	}

	message = clipRunes(message, maxChars)
	return title, summary, message
}

func runeCount(value string) int {
	return len([]rune(value))
}

// clipRunes 按 rune 截断到 limit 长度，截断后不追加省略号。通知正文只要求
// 不超过渠道限长，加省略号反而会挤占正文空间；非正数 limit 返回空串。
func clipRunes(value string, limit int) string {
	if limit <= 0 {
		return ""
	}
	runes := []rune(value)
	if len(runes) <= limit {
		return value
	}
	return string(runes[:limit])
}
