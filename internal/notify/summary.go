package notify

import (
	"regexp"
	"strings"
)

const minSentenceBoundaryRunes = 100

var (
	reCodeBlock    = regexp.MustCompile("(?s)```.*?```")
	reManyBreaks   = regexp.MustCompile(`\n{3,}`)
	reHeading      = regexp.MustCompile(`(?m)^#{1,6}\s+`)
	reListMarker   = regexp.MustCompile(`(?m)^\s*(?:[-*]|\d+[.)])\s+`)
	reInlineBullet = regexp.MustCompile(`([^\r\n\s])\s*•\s*`)
)

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

// FormatNotifySummary renders Markdown-ish agent output as readable plain text
// for ClawBot. Code blocks are omitted because proactive chat notifications are
// more useful when they stay concise.
func FormatNotifySummary(text string, maxChars int) string {
	text = strings.TrimPrefix(text, "\uFEFF")
	text = strings.ReplaceAll(text, "\r\n", "\n")
	text = strings.ReplaceAll(text, "\r", "\n")
	text = reCodeBlock.ReplaceAllString(text, "")
	text = reHeading.ReplaceAllString(text, "")
	text = reListMarker.ReplaceAllString(text, "• ")
	text = reInlineBullet.ReplaceAllString(text, "$1\n• ")

	lines := strings.Split(strings.TrimSpace(text), "\n")
	for i, line := range lines {
		lines[i] = strings.TrimSpace(line)
	}
	text = strings.TrimSpace(strings.Join(lines, "\n"))
	text = reManyBreaks.ReplaceAllString(text, "\n\n")
	text = cleanTrailingSummaryMarkers(text)
	return CutSentence(text, maxChars)
}

func cleanTrailingSummaryMarkers(text string) string {
	for {
		orig := text
		text = strings.TrimRight(text, "-\r\n\t ")
		text = strings.TrimSpace(text)
		lines := strings.Split(text, "\n")
		if len(lines) > 0 {
			last := strings.TrimSpace(lines[len(lines)-1])
			lastClean := strings.TrimPrefix(last, ">")
			lastClean = strings.TrimSpace(lastClean)
			if strings.Contains(lastClean, "微信直接引用此消息可继续对话") {
				text = strings.TrimSpace(strings.Join(lines[:len(lines)-1], "\n"))
			}
		}
		text = strings.TrimRight(text, "-\r\n\t ")
		text = strings.TrimSpace(text)
		if text == orig {
			break
		}
	}
	return text
}
