package config

import (
	"os"
	"path/filepath"
	"testing"
	"time"
)

func TestDefaultConfig(t *testing.T) {
	t.Setenv("AGENT_NOTIFY_QUIET", "22-7")
	t.Setenv("AGENT_NOTIFY_COOLDOWN_MIN", "15")

	cfg := DefaultConfig()
	if cfg.QuietHours != "22-7" {
		t.Fatalf("QuietHours = %q, want 22-7", cfg.QuietHours)
	}
	if cfg.CooldownMin != 15 {
		t.Fatalf("CooldownMin = %d, want 15", cfg.CooldownMin)
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
}
