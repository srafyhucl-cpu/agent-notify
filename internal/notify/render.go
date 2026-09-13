package notify

import (
	"strings"
	"time"
)

const (
	notificationSeparator = "\n\n"

	codexTitlePrefix    = "【codex】"
	openCodeTitlePrefix = "【opencode】"
	genericTitlePrefix  = "【通知】"

	codexDefaultTitle    = "跑完了"
	openCodeDefaultTitle = "opencode会话"
	genericDefaultTitle  = "任务完成"
	defaultBody          = "任务已完成。"

	footerTimeLayout = "2006/01/02 15:04"
)

type renderedNotification struct {
	Title   string
	Summary string
	Message string
}

func renderNotification(opts NotifyOptions, now time.Time) renderedNotification {
	title := normalizeNotificationTitle(opts.Agent, opts.Title)
	summary := FormatNotifySummary(opts.Summary, 0)
	if summary == "" {
		summary = defaultBody
	}
	if notice := strings.TrimSpace(opts.Notice); notice != "" {
		summary = strings.TrimSpace(summary + notificationSeparator + notice)
	}
	footer := notificationFooter(opts.Agent, now)

	message := composeNotification(title, summary, footer)
	if opts.MaxChars > 0 {
		title, summary, message = fitNotification(title, summary, footer, opts.MaxChars)
	}
	return renderedNotification{Title: title, Summary: summary, Message: message}
}

func normalizeNotificationTitle(agentName, rawTitle string) string {
	title := strings.TrimSpace(strings.ReplaceAll(rawTitle, "\n", " "))
	switch strings.ToLower(strings.TrimSpace(agentName)) {
	case "codex":
		if title == "" {
			title = codexDefaultTitle
		}
		if !strings.HasPrefix(title, codexTitlePrefix) {
			title = codexTitlePrefix + title
		}
	case "opencode":
		if title == "" {
			title = openCodeDefaultTitle
		}
		if !strings.HasPrefix(title, openCodeTitlePrefix) {
			title = openCodeTitlePrefix + title
		}
	default:
		if title == "" {
			title = genericDefaultTitle
		}
		if !strings.HasPrefix(title, "【") {
			title = genericTitlePrefix + title
		}
	}
	return title
}

func notificationFooter(agentName string, now time.Time) string {
	switch strings.ToLower(strings.TrimSpace(agentName)) {
	case "codex":
		return "Codex · " + now.Format(footerTimeLayout)
	case "opencode":
		return "OpenCode · " + now.Format(footerTimeLayout)
	default:
		return ""
	}
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

	message = truncateRunes(message, maxChars)
	return title, summary, message
}

func runeCount(value string) int {
	return len([]rune(value))
}

func truncateRunes(value string, limit int) string {
	if limit <= 0 {
		return ""
	}
	runes := []rune(value)
	if len(runes) <= limit {
		return value
	}
	return string(runes[:limit])
}
