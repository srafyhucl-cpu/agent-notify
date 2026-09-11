package notify

import (
	"strings"
	"testing"
)

func TestFormatNotifySummary(t *testing.T) {
	tests := []struct {
		name     string
		input    string
		max      int
		expected string
	}{
		{
			name:     "标题行加粗",
			input:    "# 标题",
			max:      500,
			expected: "<b>标题</b>",
		},
		{
			name:     "无序与有序列表转圆点",
			input:    "- 甲\n1. 乙",
			max:      500,
			expected: "• 甲<br>• 乙",
		},
		{
			name:     "引用行（由于先转义，> 变为 &gt;）",
			input:    "> 引用",
			max:      500,
			expected: "&gt; 引用",
		},
		{
			name:     "行内加粗",
			input:    "**加粗**",
			max:      500,
			expected: "<b>加粗</b>",
		},
		{
			name:     "行内代码去反引号",
			input:    "`x`",
			max:      500,
			expected: "x",
		},
		{
			name:     "HTML 转义",
			input:    "a < b & c",
			max:      500,
			expected: "a &lt; b &amp; c",
		},
		{
			name:     "代码块整段剔除",
			input:    "前\n```secret```\n后",
			max:      500,
			expected: "前<br><br>后",
		},
		{
			name:     "按句截断：句号在 100 字之后时切在句末",
			input:    strings.Repeat("a", 110) + "。" + strings.Repeat("b", 50),
			max:      120,
			expected: strings.Repeat("a", 110) + "。…",
		},
		{
			name:     "按句截断：无边界时硬切",
			input:    strings.Repeat("a", 200),
			max:      120,
			expected: strings.Repeat("a", 120) + "…",
		},
		{
			name:     "连续空行折叠为两行",
			input:    "a\n\n\n\nb",
			max:      500,
			expected: "a<br><br>b",
		},
		{
			name:     "--- 分隔行剔除",
			input:    "a\n---\nb",
			max:      500,
			expected: "a<br>b",
		},
		{
			name:     "首尾空白裁掉",
			input:    "\n\na\n\n",
			max:      500,
			expected: "a",
		},
		{
			name:     "普通文本原样输出",
			input:    "hello",
			max:      500,
			expected: "hello",
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			actual := FormatNotifySummary(tt.input, tt.max)
			if actual != tt.expected {
				t.Errorf("FormatNotifySummary() = %q, want %q", actual, tt.expected)
			}
		})
	}
}
