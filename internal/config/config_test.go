package config

import (
	"bytes"
	"os"
	"path/filepath"
	"sync"
	"testing"
	"time"
)

func TestDefaultConfig(t *testing.T) {
	t.Setenv("AGENT_NOTIFY_QUIET", "22-7")
	t.Setenv("AGENT_NOTIFY_COOLDOWN_MIN", "15")
	t.Setenv("AGENT_NOTIFY_REPLY_ENABLED", "true")

	cfg := DefaultConfig()
	if cfg.QuietHours != "22-7" {
		t.Fatalf("QuietHours = %q, want 22-7", cfg.QuietHours)
	}
	if cfg.CooldownMin != 15 {
		t.Fatalf("CooldownMin = %d, want 15", cfg.CooldownMin)
	}
	if !cfg.ReplyEnabled {
		t.Fatal("ReplyEnabled = false, want true")
	}
}

func TestReplyEnabledDefaultsOffForLegacyConfig(t *testing.T) {
	t.Setenv("AGENT_NOTIFY_REPLY_ENABLED", "")
	path := filepath.Join(t.TempDir(), "config.json")
	if err := os.WriteFile(path, []byte(`{"quietHours":"","cooldownMin":10}`), 0600); err != nil {
		t.Fatalf("write legacy config: %v", err)
	}

	cfg, err := LoadConfig(path)
	if err != nil {
		t.Fatalf("LoadConfig: %v", err)
	}
	if cfg.ReplyEnabled {
		t.Fatal("legacy config enabled quoted replies by default")
	}
}

// 引用送达确认默认开启、可显式关闭，且环境变量可覆盖。
func TestReplyConfirmationDefaultsOnAndCanBeDisabled(t *testing.T) {
	t.Setenv("AGENT_NOTIFY_REPLY_CONFIRMATION", "")
	if cfg := DefaultConfig(); !cfg.ReplyConfirmation {
		t.Fatal("ReplyConfirmation should default to true")
	}
	t.Setenv("AGENT_NOTIFY_REPLY_CONFIRMATION", "0")
	if cfg := DefaultConfig(); cfg.ReplyConfirmation {
		t.Fatal("env override should disable ReplyConfirmation")
	}
	t.Setenv("AGENT_NOTIFY_REPLY_CONFIRMATION", "")

	path := filepath.Join(t.TempDir(), "config.json")
	if err := os.WriteFile(path, []byte(`{"replyEnabled":true}`), 0600); err != nil {
		t.Fatalf("write legacy config: %v", err)
	}
	cfg, err := LoadConfig(path)
	if err != nil {
		t.Fatalf("LoadConfig: %v", err)
	}
	if !cfg.ReplyConfirmation {
		t.Fatal("missing replyConfirmation should default to true")
	}

	if err := os.WriteFile(path, []byte(`{"replyConfirmation":false}`), 0600); err != nil {
		t.Fatalf("write disabled config: %v", err)
	}
	cfg, err = LoadConfig(path)
	if err != nil {
		t.Fatalf("LoadConfig: %v", err)
	}
	if cfg.ReplyConfirmation {
		t.Fatal("explicit false should disable ReplyConfirmation")
	}
}

func TestLoadSaveConfig(t *testing.T) {
	t.Setenv("AGENT_NOTIFY_QUIET", "")
	t.Setenv("AGENT_NOTIFY_COOLDOWN_MIN", "")
	path := filepath.Join(t.TempDir(), "nested", "config.json")

	want := AppConfig{QuietHours: "23-8", CooldownMin: 20}
	if err := SaveConfig(want, path); err != nil {
		t.Fatalf("SaveConfig: %v", err)
	}
	got, err := LoadConfig(path)
	if err != nil {
		t.Fatalf("LoadConfig: %v", err)
	}
	if got != want {
		t.Fatalf("LoadConfig = %#v, want %#v", got, want)
	}

	replaced := AppConfig{QuietHours: "1-7", CooldownMin: 30}
	if err := SaveConfig(replaced, path); err != nil {
		t.Fatalf("SaveConfig overwrite: %v", err)
	}
	got, err = LoadConfig(path)
	if err != nil {
		t.Fatalf("LoadConfig overwrite: %v", err)
	}
	if got != replaced {
		t.Fatalf("LoadConfig after overwrite = %#v, want %#v", got, replaced)
	}
}

func TestLoadConfigMissingAndInvalid(t *testing.T) {
	path := filepath.Join(t.TempDir(), "missing.json")
	if _, err := LoadConfig(path); err != nil {
		t.Fatalf("missing config should use defaults: %v", err)
	}

	invalid := filepath.Join(t.TempDir(), "invalid.json")
	if err := os.WriteFile(invalid, []byte("{"), 0600); err != nil {
		t.Fatal(err)
	}
	if _, err := LoadConfig(invalid); err == nil {
		t.Fatal("invalid config should return an error")
	}
}

func TestIsInQuietHours(t *testing.T) {
	tests := []struct {
		name  string
		quiet string
		hour  int
		want  bool
	}{
		{"empty", "", 12, false},
		{"daytime inside", "9-17", 10, true},
		{"daytime end exclusive", "9-17", 17, false},
		{"overnight before end", "23-8", 2, true},
		{"overnight after start", "23-8", 23, true},
		{"overnight outside", "23-8", 12, false},
		{"invalid", "8-8", 8, false},
	}
	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			now := time.Date(2026, 9, 11, tt.hour, 0, 0, 0, time.Local)
			if got := IsInQuietHours(tt.quiet, now); got != tt.want {
				t.Fatalf("IsInQuietHours(%q, %d) = %v, want %v", tt.quiet, tt.hour, got, tt.want)
			}
		})
	}
}

func TestGetPathsHonorsOverrides(t *testing.T) {
	dir := t.TempDir()
	t.Setenv("AGENT_NOTIFY_CONFIG_DIR", dir)
	t.Setenv("AGENT_NOTIFY_TEMP_DIR", filepath.Join(dir, "temp"))
	paths := GetPaths()
	if paths.ConfigFile != filepath.Join(dir, "config.json") {
		t.Fatalf("ConfigFile = %q", paths.ConfigFile)
	}
	if paths.CredentialFile != filepath.Join(dir, "clawbot.json") {
		t.Fatalf("CredentialFile = %q", paths.CredentialFile)
	}
	if paths.SetupStateFile != filepath.Join(dir, "setup-state.json") {
		t.Fatalf("SetupStateFile = %q", paths.SetupStateFile)
	}
}

func TestGetPathsUsesCurrentUserProfileByDefault(t *testing.T) {
	home := t.TempDir()
	tempRoot := filepath.Join(home, "Temp")
	t.Setenv("USERPROFILE", home)
	t.Setenv("TEMP", tempRoot)
	for _, key := range []string{
		"AGENT_NOTIFY_REPLY_ROUTE_FILE",
		"AGENT_NOTIFY_REPLY_STATE_FILE",
		"AGENT_NOTIFY_OPENCODE_REPLY_DIR",
		"AGENT_NOTIFY_CONFIG_DIR",
		"AGENT_NOTIFY_TEMP_DIR",
		"AGENT_NOTIFY_CONFIG_FILE",
		"AGENT_NOTIFY_CREDENTIAL_FILE",
		"AGENT_NOTIFY_PLUGIN_FILE",
		"AGENT_NOTIFY_OPENCODE_MARKER_FILE",
		"AGENT_NOTIFY_CODEX_MARKER_FILE",
		"AGENT_NOTIFY_DEVIN_REPLY_DIR",
		"AGENT_NOTIFY_LOG_FILE",
	} {
		t.Setenv(key, "")
	}

	paths := GetPaths()
	configDir := filepath.Join(home, ".config", "agent-notify")
	if paths.ConfigDir != configDir {
		t.Fatalf("ConfigDir = %q, want %q", paths.ConfigDir, configDir)
	}
	if paths.CredentialFile != filepath.Join(configDir, "clawbot.json") {
		t.Fatalf("CredentialFile = %q", paths.CredentialFile)
	}
	if paths.ReplyRouteFile != filepath.Join(configDir, "reply-routes.jsonl") {
		t.Fatalf("ReplyRouteFile = %q", paths.ReplyRouteFile)
	}
	if paths.ReplyStateFile != filepath.Join(configDir, "reply-state.jsonl") {
		t.Fatalf("ReplyStateFile = %q", paths.ReplyStateFile)
	}
	if paths.OpenCodeReplyDir != filepath.Join(configDir, "opencode-reply-inbox") {
		t.Fatalf("OpenCodeReplyDir = %q", paths.OpenCodeReplyDir)
	}
	if paths.DevinReplyDir != filepath.Join(configDir, "devin-reply-inbox") {
		t.Fatalf("DevinReplyDir = %q", paths.DevinReplyDir)
	}
	if paths.PluginFile != filepath.Join(home, ".config", "opencode", "plugins", "agent-notify.ts") {
		t.Fatalf("PluginFile = %q", paths.PluginFile)
	}
	if paths.TempDir != filepath.Join(tempRoot, "agent-notify") {
		t.Fatalf("TempDir = %q", paths.TempDir)
	}
}

// 并发写同一路径时，临时文件必须唯一，最终内容应是某一次完整写入。
// Windows 上并发替换同一目标文件时，部分 writer 的 rename 会明确失败，
// 这是可接受的显式失败；这里只校验「不损坏」：至少一个成功、内容完整、无残留临时文件。
func TestWriteFileAtomicConcurrentWriters(t *testing.T) {
	dir := t.TempDir()
	path := filepath.Join(dir, "config.json")

	const writers = 16
	payload := func(i int) []byte {
		return bytes.Repeat([]byte{byte('a' + i)}, 4096)
	}

	var wg sync.WaitGroup
	var mu sync.Mutex
	successCount := 0
	for i := 0; i < writers; i++ {
		wg.Add(1)
		go func(i int) {
			defer wg.Done()
			if err := writeFileAtomic(path, payload(i), 0600); err == nil {
				mu.Lock()
				successCount++
				mu.Unlock()
			}
		}(i)
	}
	wg.Wait()

	if successCount < 1 {
		t.Fatal("no writer succeeded, want at least one")
	}

	// 最终内容必须是某一次写入的完整载荷，证明没有交叉写入或截断。
	data, err := os.ReadFile(path)
	if err != nil {
		t.Fatal(err)
	}
	matched := false
	for i := 0; i < writers; i++ {
		if bytes.Equal(data, payload(i)) {
			matched = true
			break
		}
	}
	if !matched {
		t.Fatalf("final content length %d does not match any writer payload", len(data))
	}

	leftovers, err := filepath.Glob(path + ".*.tmp")
	if err != nil {
		t.Fatal(err)
	}
	if len(leftovers) != 0 {
		t.Fatalf("temporary files remain: %#v", leftovers)
	}
}

func TestThemeConfig(t *testing.T) {
	path := filepath.Join(t.TempDir(), "config_theme.json")

	cfg := AppConfig{Theme: "light"}
	if err := SaveConfig(cfg, path); err != nil {
		t.Fatalf("SaveConfig: %v", err)
	}
	loaded, err := LoadConfig(path)
	if err != nil {
		t.Fatalf("LoadConfig: %v", err)
	}
	if loaded.Theme != "light" {
		t.Fatalf("loaded theme = %q, want light", loaded.Theme)
	}

	norm := NormalizeConfig(AppConfig{Theme: "INVALID"})
	if norm.Theme != "" {
		t.Fatalf("invalid theme normalized = %q, want empty", norm.Theme)
	}
}
