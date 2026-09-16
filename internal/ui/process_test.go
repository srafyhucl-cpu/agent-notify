//go:build windows

package ui

import "testing"

// 结束残留实例时必须按目录精确匹配：D:\bin 不能匹配 D:\bin2。
func TestSameProgramDir(t *testing.T) {
	cases := []struct {
		name      string
		selfDir   string
		otherPath string
		want      bool
	}{
		{"同目录", `D:\bin`, `D:\bin\agent-notify.exe`, true},
		{"同目录大小写不同", `D:\Bin`, `d:\bin\AGENT-NOTIFY.EXE`, true},
		{"结尾分隔符", `D:\bin\`, `D:\bin\agent-notify.exe`, true},
		{"前缀相似的兄弟目录", `D:\bin`, `D:\bin2\agent-notify.exe`, false},
		{"子目录不是同目录", `D:\bin`, `D:\bin\sub\agent-notify.exe`, false},
		{"空路径", `D:\bin`, ``, false},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			if got := sameProgramDir(tc.selfDir, tc.otherPath); got != tc.want {
				t.Fatalf("sameProgramDir(%q, %q) = %v, want %v", tc.selfDir, tc.otherPath, got, tc.want)
			}
		})
	}
}
