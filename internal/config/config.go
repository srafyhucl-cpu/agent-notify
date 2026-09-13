package config

import (
	"encoding/json"
	"fmt"
	"os"
	"path/filepath"
	"regexp"
	"strconv"
	"strings"
	"time"
)

const (
	DefaultCooldownMin = 10
	maxCooldownMinutes = 24 * 60
	minClockHour       = 0
	maxClockHour       = 23
)

type AppConfig struct {
	QuietHours   string `json:"quietHours"`
	CooldownMin  int    `json:"cooldownMin"`
	ReplyEnabled bool   `json:"replyEnabled"`
}

var quietHoursPattern = regexp.MustCompile(`^\s*(\d{1,2})\s*-\s*(\d{1,2})\s*$`)

func DefaultConfig() AppConfig {
	cfg := AppConfig{
		QuietHours:  strings.TrimSpace(os.Getenv("AGENT_NOTIFY_QUIET")),
		CooldownMin: DefaultCooldownMin,
	}
	if raw := strings.TrimSpace(os.Getenv("AGENT_NOTIFY_REPLY_ENABLED")); raw != "" {
		cfg.ReplyEnabled = raw == "1" || strings.EqualFold(raw, "true") || strings.EqualFold(raw, "on")
	}
	if raw := strings.TrimSpace(os.Getenv("AGENT_NOTIFY_COOLDOWN_MIN")); raw != "" {
		if value, err := strconv.Atoi(raw); err == nil && value > 0 {
			cfg.CooldownMin = value
		}
	}
	return cfg
}

// LoadConfig reads the Agent-notify configuration. A missing file is valid and
// returns environment-backed defaults; malformed JSON is returned as an error.
func LoadConfig(configPath string) (AppConfig, error) {
	cfg := DefaultConfig()
	if strings.TrimSpace(configPath) == "" {
		configPath = GetPaths().ConfigFile
	}

	data, err := os.ReadFile(configPath)
	if err != nil {
		if os.IsNotExist(err) {
			return cfg, nil
		}
		return cfg, fmt.Errorf("read config: %w", err)
	}

	var raw struct {
		QuietHours   *string `json:"quietHours"`
		CooldownMin  *int    `json:"cooldownMin"`
		ReplyEnabled *bool   `json:"replyEnabled"`
	}
	if err := json.Unmarshal(data, &raw); err != nil {
		return cfg, fmt.Errorf("decode config: %w", err)
	}
	if raw.QuietHours != nil {
		cfg.QuietHours = strings.TrimSpace(*raw.QuietHours)
	}
	if raw.CooldownMin != nil && *raw.CooldownMin > 0 {
		cfg.CooldownMin = *raw.CooldownMin
	}
	if raw.ReplyEnabled != nil {
		cfg.ReplyEnabled = *raw.ReplyEnabled
	}
	return NormalizeConfig(cfg), nil
}

func NormalizeConfig(cfg AppConfig) AppConfig {
	cfg.QuietHours = strings.TrimSpace(cfg.QuietHours)
	if !ValidQuietHours(cfg.QuietHours) {
		cfg.QuietHours = ""
	}
	if cfg.CooldownMin <= 0 {
		cfg.CooldownMin = DefaultCooldownMin
	}
	if cfg.CooldownMin > maxCooldownMinutes {
		cfg.CooldownMin = maxCooldownMinutes
	}
	return cfg
}

// SaveConfig writes the configuration atomically.
func SaveConfig(cfg AppConfig, configPath string) error {
	cfg = NormalizeConfig(cfg)
	if strings.TrimSpace(configPath) == "" {
		configPath = GetPaths().ConfigFile
	}

	if err := os.MkdirAll(filepath.Dir(configPath), 0700); err != nil {
		return fmt.Errorf("create config directory: %w", err)
	}
	data, err := json.MarshalIndent(cfg, "", "  ")
	if err != nil {
		return fmt.Errorf("encode config: %w", err)
	}
	return writeFileAtomic(configPath, append(data, '\n'), 0600)
}

func writeFileAtomic(path string, data []byte, mode os.FileMode) error {
	tmp := path + ".tmp"
	if err := os.WriteFile(tmp, data, mode); err != nil {
		return err
	}
	if err := os.Rename(tmp, path); err != nil {
		_ = os.Remove(tmp)
		return err
	}
	return nil
}

func ValidQuietHours(raw string) bool {
	raw = strings.TrimSpace(raw)
	if raw == "" {
		return true
	}
	match := quietHoursPattern.FindStringSubmatch(raw)
	if len(match) != 3 {
		return false
	}
	start, errStart := strconv.Atoi(match[1])
	end, errEnd := strconv.Atoi(match[2])
	return errStart == nil && errEnd == nil &&
		start >= minClockHour && start <= maxClockHour &&
		end >= minClockHour && end <= maxClockHour && start != end
}

// IsInQuietHours reports whether t falls in the configured start-end window.
// The start is inclusive and the end is exclusive; a start after the end wraps
// across midnight.
func IsInQuietHours(raw string, t time.Time) bool {
	if !ValidQuietHours(raw) || strings.TrimSpace(raw) == "" {
		return false
	}
	match := quietHoursPattern.FindStringSubmatch(strings.TrimSpace(raw))
	start, _ := strconv.Atoi(match[1])
	end, _ := strconv.Atoi(match[2])
	hour := t.Hour()
	if start < end {
		return hour >= start && hour < end
	}
	return hour >= start || hour < end
}
