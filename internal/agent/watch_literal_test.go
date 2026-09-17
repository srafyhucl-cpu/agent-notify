package agent

import (
	"os"
	"path/filepath"
	"strings"
	"testing"
)

// TestHandleWatchKeepsLiteralExePathWithDollar 回归用例：安装路径里含 `$` 时，
// HandleWatch 用 ReplaceAllString 替换 notify 行会把 `$weird` 当成正则分组引用
// 展开成空串，导致写回的路径被吞掉。
//
// 该用例当前预期失败，用来暴露真实 bug；修复方向见报告（ReplaceAllLiteralString）。
func TestHandleWatchKeepsLiteralExePathWithDollar(t *testing.T) {
	tempDir := t.TempDir()
	t.Setenv("AGENT_NOTIFY_CONFIG_DIR", filepath.Join(tempDir, "config"))
	t.Setenv("AGENT_NOTIFY_TEMP_DIR", filepath.Join(tempDir, "temp"))

	configPath := filepath.Join(tempDir, "config.toml")
	original := "model = \"test\"\nnotify = [ \"C:/some/path/codex-computer-use.exe\", \"turn-ended\" ]\n"
	if err := os.WriteFile(configPath, []byte(original), 0600); err != nil {
		t.Fatal(err)
	}

	fakeExe := `C:\Tools\$weird\agent-notify.exe`
	if err := HandleWatch(configPath, fakeExe); err != nil {
		t.Fatalf("HandleWatch: %v", err)
	}
	data, err := os.ReadFile(configPath)
	if err != nil {
		t.Fatal(err)
	}

	want := `notify = [ "C:/Tools/$weird/agent-notify.exe", "codex", "turn-ended" ]`
	if !strings.Contains(string(data), want) {
		t.Fatalf("exePath 中的 $ 被正则展开，写回的不是字面路径:\n写回内容:\n%s\n期望包含:\n%s", data, want)
	}
	if strings.Contains(string(data), `C:/Tools//agent-notify.exe`) {
		t.Fatalf("路径片段 $weird 被吞掉:\n%s", data)
	}
}
