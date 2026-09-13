package reply

// truncateRunes keeps at most max runes and appends an ellipsis when the value
// was shortened. A non-positive limit yields an empty string so callers cannot
// accidentally forward an unbounded message to WeChat.
func truncateRunes(value string, max int) string {
	if max <= 0 {
		return ""
	}
	runes := []rune(value)
	if len(runes) <= max {
		return value
	}
	return string(runes[:max]) + "…"
}
