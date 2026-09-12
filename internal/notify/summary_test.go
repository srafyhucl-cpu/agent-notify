package notify

import (
	"strings"
	"testing"
)

func TestFormatNotifySummary(t *testing.T) {
	tests := []struct {
		name string
		in   string
		max  int
		want string
	}{
		{"heading", "# 标题", 500, "标题"},
		{"lists", "- 甲\n1. 乙", 500, "• 甲\n• 乙"},
		{"inline markup", "**加粗** 和 `code`", 500, "加粗 和 code"},
		{"code block omitted", "前\n```secret```\n后", 500, "前\n\n后"},
		{"collapse blank lines", "a\n\n\n\nb", 500, "a\n\nb"},
		{"plain text", "hello", 500, "hello"},
	}
	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			if got := FormatNotifySummary(tt.in, tt.max); got != tt.want {
				t.Fatalf("FormatNotifySummary() = %q, want %q", got, tt.want)
			}
		})
	}
}

func TestCutSentence(t *testing.T) {
	tests := []struct {
		name string
		in   string
		max  int
		want string
	}{
		{"short", "hello", 10, "hello"},
		{"hard cut", strings.Repeat("a", 200), 100, strings.Repeat("a", 100) + "…"},
		{"sentence cut", strings.Repeat("a", 110) + "。" + strings.Repeat("b", 50), 120, strings.Repeat("a", 110) + "。…"},
	}
	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			if got := CutSentence(tt.in, tt.max); got != tt.want {
				t.Fatalf("CutSentence() = %q, want %q", got, tt.want)
			}
		})
	}
}
