package agent

import (
	"os"
	"path/filepath"
	"strings"
	"testing"
)

func TestHandleWatch(t *testing.T) {
	tempDir := t.TempDir()
	t.Setenv("AGENT_NOTIFY_CONFIG_DIR", filepath.Join(tempDir, "config"))
	t.Setenv("AGENT_NOTIFY_TEMP_DIR", filepath.Join(tempDir, "temp"))

	configPath := filepath.Join(tempDir, "config.toml")
	original := "model = \"test\"\nnotify = [ \"C:/some/path/codex-computer-use.exe\", \"turn-ended\" ]\n"
	if err := os.WriteFile(configPath, []byte(original), 0600); err != nil {
		t.Fatal(err)
	}

	fakeExe := `D:\Tools\Agent-notify\agent-notify.exe`
	if err := HandleWatch(configPath, fakeExe); err != nil {
		t.Fatalf("HandleWatch: %v", err)
	}
	data, err := os.ReadFile(configPath)
	if err != nil {
		t.Fatal(err)
	}
	expected := `notify = [ "D:/Tools/Agent-notify/agent-notify.exe", "codex", "turn-ended" ]`
	if !strings.Contains(string(data), expected) {
		t.Fatalf("patched config:\n%s\nwant line:\n%s", data, expected)
	}
	if _, err := os.Stat(configPath + ".bak-notify-wrapper"); err != nil {
		t.Fatalf("backup not created: %v", err)
	}

	if err := HandleWatch(configPath, fakeExe); err != nil {
		t.Fatalf("second HandleWatch: %v", err)
	}
	data2, _ := os.ReadFile(configPath)
	if string(data2) != string(data) {
		t.Fatalf("HandleWatch is not idempotent")
	}

	customPath := filepath.Join(tempDir, "custom.toml")
	custom := "notify = [ \"my-custom-logger\", \"event\" ]\n"
	if err := os.WriteFile(customPath, []byte(custom), 0600); err != nil {
		t.Fatal(err)
	}
	if err := HandleWatch(customPath, fakeExe); err != nil {
		t.Fatalf("custom HandleWatch: %v", err)
	}
	customAfter, _ := os.ReadFile(customPath)
	if string(customAfter) != custom {
		t.Fatalf("custom notify was modified: %s", customAfter)
	}
}

func TestHandleWatchIgnoresAgentNotifyOutsideNotifyLine(t *testing.T) {
	tempDir := t.TempDir()
	t.Setenv("AGENT_NOTIFY_CONFIG_DIR", filepath.Join(tempDir, "config"))
	t.Setenv("AGENT_NOTIFY_TEMP_DIR", filepath.Join(tempDir, "temp"))

	configPath := filepath.Join(tempDir, "config.toml")
	original := `[projects.'d:\project\agent-notify']
trust_level = "trusted"
notify = [ "C:/some/path/codex-computer-use.exe", "turn-ended", "--previous-notify", "[\"D:/app/linkWeixin/linkweixin.exe\",\"codex\",\"turn-ended\"]" ]
`
	if err := os.WriteFile(configPath, []byte(original), 0600); err != nil {
		t.Fatal(err)
	}

	if err := HandleWatch(configPath, `D:\Tools\Agent-notify\agent-notify.exe`); err != nil {
		t.Fatalf("HandleWatch: %v", err)
	}
	data, err := os.ReadFile(configPath)
	if err != nil {
		t.Fatal(err)
	}
	expected := `notify = [ "D:/Tools/Agent-notify/agent-notify.exe", "codex", "turn-ended" ]`
	if !strings.Contains(string(data), expected) {
		t.Fatalf("patched config:\n%s\nwant line:\n%s", data, expected)
	}
	if strings.Contains(string(data), "linkWeixin") {
		t.Fatalf("old notify chain survived:\n%s", data)
	}
}
