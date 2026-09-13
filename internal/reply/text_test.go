package reply

import "testing"

func TestTruncateRunes(t *testing.T) {
	tests := []struct {
		name  string
		value string
		max   int
		want  string
	}{
		{name: "empty", value: "", max: 4, want: ""},
		{name: "shorter than limit", value: "abc", max: 4, want: "abc"},
		{name: "exact limit", value: "abcd", max: 4, want: "abcd"},
		{name: "over limit keeps rune boundary", value: "失败详情很长", max: 3, want: "失败详…"},
		{name: "non positive limit yields empty", value: "abc", max: 0, want: ""},
	}
	for _, test := range tests {
		t.Run(test.name, func(t *testing.T) {
			if got := truncateRunes(test.value, test.max); got != test.want {
				t.Fatalf("truncateRunes(%q, %d) = %q, want %q", test.value, test.max, got, test.want)
			}
		})
	}
}
