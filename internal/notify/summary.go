package notify

import (
	"regexp"
	"strings"
)

const minSentenceBoundaryRunes = 100

var reManyBreaks = regexp.MustCompile(`\n{3,}`)

// CutSentence truncates text at a sentence boundary within max runes when a
// useful boundary is available.
func CutSentence(text string, maxChars int) string {
	if maxChars <= 0 {
		return text
	}
	runes := []rune(text)
	if len(runes) <= maxChars {
		return text
	}
	window := runes[:maxChars]
	idx := -1
	for i := len(window) - 1; i >= 0; i-- {
		switch window[i] {
		case '。', '！', '？', '!', '?', '\n':
			idx = i
		}
		if idx >= 0 {
			break
		}
	}
	if idx >= minSentenceBoundaryRunes {
		return strings.TrimSpace(string(window[:idx+1])) + "…"
	}
	return strings.TrimSpace(string(window)) + "…"
}

// FormatNotifySummary 整理 Agent 的 Markdown 输出供 ClawBot 渲染。
//
// ClawBot 聊天界面支持 Markdown（标题、列表、代码块、表格、引用、粗体），因此这里只做
// 无损整理：去 BOM、统一换行、去行尾空白、压缩多余空行。不再删除代码块、剥离标题号或
// 改写列表符号——那会把 Markdown 破坏成"半 Markdown 的混乱文本"，还会在纯文本客户端丢信息。
func FormatNotifySummary(text string, maxChars int) string {
	text = strings.TrimPrefix(text, "\uFEFF")
	text = strings.ReplaceAll(text, "\r\n", "\n")
	text = strings.ReplaceAll(text, "\r", "\n")

	lines := strings.Split(text, "\n")
	for i, line := range lines {
		lines[i] = strings.TrimRight(line, " \t")
	}
	text = strings.TrimSpace(strings.Join(lines, "\n"))
	text = reManyBreaks.ReplaceAllString(text, "\n\n")
	text = cleanTrailingSummaryMarkers(text)
	return CutSentence(text, maxChars)
}

// cleanTrailingSummaryMarkers 只去掉末尾的噪音：空行、Agent 误带的旧页脚（引用提示行）
// 与纯分隔线行。绝不改动正文里的 Markdown（例如结尾的 ** 加粗、列表项）。
func cleanTrailingSummaryMarkers(text string) string {
	lines := strings.Split(strings.TrimSpace(text), "\n")
	for len(lines) > 0 {
		last := strings.TrimSpace(lines[len(lines)-1])
		switch {
		case last == "":
			lines = lines[:len(lines)-1]
		case strings.Contains(last, replyHintText):
			lines = lines[:len(lines)-1]
		case isDividerLine(last):
			lines = lines[:len(lines)-1]
		default:
			return strings.TrimSpace(strings.Join(lines, "\n"))
		}
	}
	return ""
}

// isDividerLine 判断一行是否只由 Markdown 分隔符组成（--- / — / *** 等）。
func isDividerLine(line string) bool {
	trimmed := strings.TrimSpace(line)
	if trimmed == "" {
		return false
	}
	for _, char := range trimmed {
		if char != '-' && char != '—' && char != '*' {
			return false
		}
	}
	return true
}
