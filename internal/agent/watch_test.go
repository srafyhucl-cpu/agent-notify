package agent

import (
	"os"
	"path/filepath"
	"strings"
	"testing"
)

func TestHandleWatch(t *testing.T) {
	tempDir := t.TempDir()
	configPath := filepath.Join(tempDir, "config.toml")

	// Case 1: Overwritten by codex with direct codex-computer-use.exe
	orig := "model = \"test\"\nnotify = [ \"C:/some/path/codex-computer-use.exe\", \"turn-ended\" ]\n"
	if err := os.WriteFile(configPath, []byte(orig), 0644); err != nil {
		t.Fatalf("failed to write test config: %v", err)
	}

	fakeExe := "D:\\app\\linkWeixin\\linkweixin.exe"
	HandleWatch(configPath, fakeExe)

	data, err := os.ReadFile(configPath)
	if err != nil {
		t.Fatalf("failed to read patched config: %v", err)
	}
	content := string(data)
	expected := "notify = [ \"D:/app/linkWeixin/linkweixin.exe\", \"codex\", \"turn-ended\" ]"
	if !strings.Contains(content, expected) {
		t.Errorf("content does not contain expected line.\nGot:\n%s\nWant line:\n%s", content, expected)
	}

	// Verify backup file created
	bakPath := configPath + ".bak-notify-wrapper"
	if _, err := os.Stat(bakPath); os.IsNotExist(err) {
		t.Errorf("backup file %s was not created", bakPath)
	}

	// Case 2: Idempotent - running again should keep content unchanged
	HandleWatch(configPath, fakeExe)
	data2, _ := os.ReadFile(configPath)
	if string(data2) != content {
		t.Errorf("HandleWatch is not idempotent: before=%s, after=%s", content, string(data2))
	}

	// Case 3: Custom notify - should NOT be modified
	customConfigPath := filepath.Join(tempDir, "custom.toml")
	customContent := "notify = [ \"my-custom-logger\", \"event\" ]\n"
	_ = os.WriteFile(customConfigPath, []byte(customContent), 0644)
	HandleWatch(customConfigPath, fakeExe)
	data3, _ := os.ReadFile(customConfigPath)
	if string(data3) != customContent {
		t.Errorf("HandleWatch modified custom notify: %s", string(data3))
	}
}
