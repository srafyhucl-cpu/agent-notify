package notify

import (
	"strings"
	"time"

	"github.com/srafyhucl-cpu/agent-notify/internal/agentmeta"
)

const (
	notificationSeparator = "\n\n"
	genericTitlePrefix    = "【通知】"
	genericDefaultTitle   = "任务已完成"
	defaultBody           = "任务已完成。"
	footerTimeLayout      = "2006/01/02 15:04"
)

type renderedNotification struct {
	Title   string
	Summary string
	Message string
}

func renderNotification(opts NotifyOptions, now time.Time) renderedNotification {
	title := normalizeNotificationTitle(opts.Agent, opts.Title, opts.Notice)
	summary := FormatNotifySummary(opts.Summary, 0)
	if summary == "" {
		summary = defaultBody
	}
	if notice := strings.TrimSpace(opts.Notice); notice != "" {
		formattedNotice := "> ⚠️ " + strings.ReplaceAll(notice, "\n", "\n> ")
		summary = strings.TrimSpace(summary + notificationSeparator + formattedNotice)
	}
	footer := notificationFooter(opts.Agent, now)

	message := composeNotification(title, summary, footer)
	if opts.MaxChars > 0 {
		title, summary, message = fitNotification(title, summary, footer, opts.MaxChars)
	}
	return renderedNotification{Title: title, Summary: summary, Message: message}
}

func normalizeNotificationTitle(agentName, rawTitle, notice string) string {
	title := strings.TrimSpace(strings.ReplaceAll(rawTitle, "\n", " "))
	badge := "🟢"
	if strings.Contains(notice, "错误") || strings.Contains(notice, "失败") || strings.Contains(notice, "异常") ||
		strings.Contains(title, "错误") || strings.Contains(title, "失败") || strings.Contains(title, "异常") {
		badge = "⚠️"
	}

	if descriptor, ok := agentmeta.Lookup(agentName); ok {
		// 剥离可能存在的各种旧前缀（大小写不限）
		clean := strings.TrimSpace(title)
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

		if hasStatusBadge(clean) {
			return clean
		}
		return badge + descriptor.TitlePrefix + clean
	}

	if title == "" || title == genericDefaultTitle || title == "任务完成" || title == "跑完了" {
		title = genericDefaultTitle
	}
	clean := strings.TrimPrefix(title, genericTitlePrefix)
	clean = strings.TrimPrefix(clean, "【通知】")
	if clean == "" {
		clean = genericDefaultTitle
	}

	if hasStatusBadge(clean) {
		return clean
	}
	return badge + genericTitlePrefix + clean
}

func hasStatusBadge(s string) bool {
	return strings.HasPrefix(s, "🟢") || strings.HasPrefix(s, "⚠️") || strings.HasPrefix(s, "🔴") || strings.HasPrefix(s, "⚡")
}

func notificationFooter(agentName string, now time.Time) string {
	descriptor, ok := agentmeta.Lookup(agentName)
	if !ok || descriptor.FooterLabel == "" {
		return ""
	}
	var sb strings.Builder
	sb.WriteString("---\n")
	if descriptor.Replyable {
		sb.WriteString("> 微信直接引用此消息可继续对话\n\n")
	}
	sb.WriteString(descriptor.FooterLabel + " · " + now.Format(footerTimeLayout))
	return sb.String()
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
