package notify

import (
	"html"
	"regexp"
	"strings"
)

var (
	reCodeBlock  = regexp.MustCompile("(?s)```.*?```")
	reHeader     = regexp.MustCompile(`^(#{1,4})\s+(.*)$`)
	reQuote      = regexp.MustCompile(`^>\s?(.*)$`)
	reList       = regexp.MustCompile(`^(\d+[.)]|[-*])\s+(.*)$`)
	reInlineCode = regexp.MustCompile("`([^`]+)`")
	reBold       = regexp.MustCompile(`\*\*(.+?)\*\*`)
	reTripleBr   = regexp.MustCompile(`(?i)(<br>\s*){3,}`)
	reTrimBr     = regexp.MustCompile(`(?i)^(<br>\s*)+|(<br>\s*)+$`)
)

// CutSentence truncates text at a punctuation boundary within max runes.
func CutSentence(text string, maxChars int) string {
	runes := []rune(text)
	if len(runes) <= maxChars {
		return text
	}
	window := runes[:maxChars]
	idx := -1
	delims := []rune{'。', '！', '？', '!', '?', '\n'}
	for i := len(window) - 1; i >= 0; i-- {
		r := window[i]
		for _, d := range delims {
			if r == d {
				idx = i
				break
			}
		}
		if idx != -1 {
			break
		}
	}
	if idx >= 100 {
		return string(window[:idx+1]) + "…"
	}
	return string(window) + "…"
}

// FormatNotifySummary renders text as HTML for PushPlus notification.
// Pure function with exact matching semantics to Format-NotifySummary.ps1.
func FormatNotifySummary(text string, maxChars int) string {
	text = strings.TrimPrefix(text, "\uFEFF")
	// 1. Remove full code blocks
	t := reCodeBlock.ReplaceAllString(text, "")

	// 2. Cut sentence before HTML encoding
	t = CutSentence(strings.TrimSpace(t), maxChars)

	// 3. HTML encode
	t = html.EscapeString(t)

	// 4. Line by line formatting
	rawLines := strings.Split(t, "\n")
	var out []string
	for _, rawLine := range rawLines {
		line := strings.TrimRight(rawLine, "\r\t ")
		if line == "---" {
			continue
		}

		l := line
		wrap := ""
		if m := reHeader.FindStringSubmatch(l); len(m) == 3 {
			wrap = "b"
			l = m[2]
		} else if m := reQuote.FindStringSubmatch(l); len(m) == 2 {
			l = m[1]
		} else if m := reList.FindStringSubmatch(l); len(m) == 3 {
			wrap = "li"
			l = m[2]
		}

		l = reInlineCode.ReplaceAllString(l, "$1")
		l = reBold.ReplaceAllString(l, "<b>$1</b>")

		switch wrap {
		case "b":
			out = append(out, "<b>"+l+"</b>")
		case "li":
			out = append(out, "• "+l)
		default:
			out = append(out, l)
		}
	}

	res := strings.Join(out, "<br>")
	res = reTripleBr.ReplaceAllString(res, "<br><br>")
	res = reTrimBr.ReplaceAllString(res, "")
	return strings.TrimSpace(res)
}
