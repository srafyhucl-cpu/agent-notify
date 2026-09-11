package config

import (
	"encoding/json"
	"os"
	"path/filepath"
	"testing"
	"time"
)

func TestDefaultConfig(t *testing.T) {
	// Clear env to get clean defaults
	t.Setenv("PUSHPLUS_TOKEN", "")
	t.Setenv("WECOM_WEBHOOK_URL", "")
	t.Setenv("FEISHU_WEBHOOK_URL", "")
	t.Setenv("DINGTALK_WEBHOOK_URL", "")
	t.Setenv("LINKWEIXIN_WEBHOOK_URL", "")
	t.Setenv("OPENCODE_NOTIFY_QUIET", "")
	t.Setenv("ANTIGRAVITY_NOTIFY_QUIET", "")
	t.Setenv("OPENCODE_NOTIFY_COOLDOWN_MIN", "")
	t.Setenv("ANTIGRAVITY_NOTIFY_COOLDOWN_MIN", "")

	cfg := DefaultConfig()
	if !cfg.Channels.PushPlus.Enabled {
		t.Error("PushPlus should be enabled by default")
	}
	if cfg.Channels.PushPlus.Token != "" {
		t.Error("Token should be empty when env is unset")
	}
	if cfg.CooldownMin != 10 {
		t.Errorf("CooldownMin = %d, want 10", cfg.CooldownMin)
	}
}

func TestDefaultConfigFromEnv(t *testing.T) {
	t.Setenv("PUSHPLUS_TOKEN", "test-token-abc")
	t.Setenv("WECOM_WEBHOOK_URL", "https://wecom.example.com/hook")
	t.Setenv("OPENCODE_NOTIFY_COOLDOWN_MIN", "5")

	cfg := DefaultConfig()
	if cfg.Channels.PushPlus.Token != "test-token-abc" {
		t.Errorf("Token = %q, want %q", cfg.Channels.PushPlus.Token, "test-token-abc")
	}
	if !cfg.Channels.WeCom.Enabled {
		t.Error("WeCom should be enabled when webhook is set")
	}
	if cfg.Channels.WeCom.Webhook != "https://wecom.example.com/hook" {
		t.Errorf("WeCom webhook mismatch")
	}
	if cfg.CooldownMin != 5 {
		t.Errorf("CooldownMin = %d, want 5", cfg.CooldownMin)
	}
}

func TestLoadConfigFromFile(t *testing.T) {
	t.Setenv("PUSHPLUS_TOKEN", "env-token")

	dir := t.TempDir()
	cfgFile := filepath.Join(dir, "config.json")

	fileToken := "file-token-123"
	cfgData := map[string]interface{}{
		"channels": map[string]interface{}{
			"pushplus": map[string]interface{}{
				"enabled": true,
				"token":   fileToken,
			},
		},
		"cooldownMin": 20,
	}
	data, _ := json.Marshal(cfgData)
	if err := os.WriteFile(cfgFile, data, 0644); err != nil {
		t.Fatalf("write config: %v", err)
	}

	cfg := LoadConfig(cfgFile)
	// File token should override env token
	if cfg.Channels.PushPlus.Token != fileToken {
		t.Errorf("Token = %q, want %q (file should override env)", cfg.Channels.PushPlus.Token, fileToken)
	}
	if cfg.CooldownMin != 20 {
		t.Errorf("CooldownMin = %d, want 20", cfg.CooldownMin)
	}
}

func TestLoadConfigPartialOverride(t *testing.T) {
	t.Setenv("PUSHPLUS_TOKEN", "env-token")
	t.Setenv("WECOM_WEBHOOK_URL", "https://wecom.example.com")

	dir := t.TempDir()
	cfgFile := filepath.Join(dir, "config.json")

	// File only overrides WeCom enabled to false, rest should keep env defaults
	cfgData := map[string]interface{}{
		"channels": map[string]interface{}{
			"wecom": map[string]interface{}{
				"enabled": false,
			},
		},
	}
	data, _ := json.Marshal(cfgData)
	_ = os.WriteFile(cfgFile, data, 0644)

	cfg := LoadConfig(cfgFile)
	// PushPlus should still have env token
	if cfg.Channels.PushPlus.Token != "env-token" {
		t.Errorf("PushPlus Token = %q, want env-token", cfg.Channels.PushPlus.Token)
	}
	// WeCom enabled should be overridden to false
	if cfg.Channels.WeCom.Enabled {
		t.Error("WeCom should be disabled per file override")
	}
	// But webhook should still have env value
	if cfg.Channels.WeCom.Webhook != "https://wecom.example.com" {
		t.Errorf("WeCom Webhook = %q, want env value", cfg.Channels.WeCom.Webhook)
	}
}

func TestLoadConfigMissingFile(t *testing.T) {
	t.Setenv("PUSHPLUS_TOKEN", "fallback-token")
	cfg := LoadConfig("/nonexistent/path/config.json")
	if cfg.Channels.PushPlus.Token != "fallback-token" {
		t.Errorf("Should fallback to env defaults, got Token = %q", cfg.Channels.PushPlus.Token)
	}
}

func TestLoadConfigInvalidJSON(t *testing.T) {
	t.Setenv("PUSHPLUS_TOKEN", "fallback-token")
	dir := t.TempDir()
	cfgFile := filepath.Join(dir, "config.json")
	_ = os.WriteFile(cfgFile, []byte("not valid json {{{"), 0644)

	cfg := LoadConfig(cfgFile)
	if cfg.Channels.PushPlus.Token != "fallback-token" {
		t.Errorf("Should fallback to env defaults on bad JSON, got Token = %q", cfg.Channels.PushPlus.Token)
	}
}

func TestSaveConfig(t *testing.T) {
	dir := t.TempDir()
	cfgFile := filepath.Join(dir, "sub", "config.json")

	cfg := AppConfig{
		Channels: ChannelsConfig{
			PushPlus: ChannelPushPlus{
				Enabled: true,
				Token:   "save-test-token",
			},
		},
		QuietHours:  "23-7",
		CooldownMin: 15,
	}

	if err := SaveConfig(cfg, cfgFile); err != nil {
		t.Fatalf("SaveConfig: %v", err)
	}

	// Verify file exists
	if _, err := os.Stat(cfgFile); err != nil {
		t.Fatalf("config file not created: %v", err)
	}

	// Reload and verify
	var loaded AppConfig
	data, _ := os.ReadFile(cfgFile)
	if err := json.Unmarshal(data, &loaded); err != nil {
		t.Fatalf("unmarshal saved config: %v", err)
	}
	if loaded.Channels.PushPlus.Token != "save-test-token" {
		t.Errorf("Loaded token = %q, want save-test-token", loaded.Channels.PushPlus.Token)
	}
	if loaded.CooldownMin != 15 {
		t.Errorf("Loaded cooldown = %d, want 15", loaded.CooldownMin)
	}
}

func TestIsInQuietHours(t *testing.T) {
	tests := []struct {
		name   string
		quiet  string
		hour   int
		expect bool
	}{
		{"empty string", "", 14, false},
		{"invalid format", "abc", 14, false},
		{"same start/end", "8-8", 8, false},
		{"daytime 9-17, hour=10", "9-17", 10, true},
		{"daytime 9-17, hour=8", "9-17", 8, false},
		{"daytime 9-17, hour=17", "9-17", 17, false},
		{"nighttime 23-8, hour=23", "23-8", 23, true},
		{"nighttime 23-8, hour=2", "23-8", 2, true},
		{"nighttime 23-8, hour=8", "23-8", 8, false},
		{"nighttime 23-8, hour=22", "23-8", 22, false},
		{"with spaces", " 23 - 8 ", 1, true},
		{"out of range hour", "25-8", 1, false},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			// Construct a time at the desired hour
			now := time.Date(2025, 6, 15, tt.hour, 30, 0, 0, time.Local)
			got := IsInQuietHours(tt.quiet, now)
			if got != tt.expect {
				t.Errorf("IsInQuietHours(%q, hour=%d) = %v, want %v", tt.quiet, tt.hour, got, tt.expect)
			}
		})
	}
}
