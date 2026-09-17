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
	// ReplyConfirmation 控制引用回复成功后是否回一条送达确认，默认开启。
	ReplyConfirmation bool   `json:"replyConfirmation"`
	DefaultAgent      string `json:"defaultAgent,omitempty"`
	WidgetAgentMode   string `json:"widgetAgentMode,omitempty"`
	Theme             string `json:"theme,omitempty"`
}

var quietHoursPattern = regexp.MustCompile(`^\s*(\d{1,2})\s*-\s*(\d{1,2})\s*$`)

func DefaultConfig() AppConfig {
	cfg := AppConfig{
		QuietHours:        strings.TrimSpace(os.Getenv("AGENT_NOTIFY_QUIET")),
		CooldownMin:       DefaultCooldownMin,
		ReplyConfirmation: true,
	}
	if raw := strings.TrimSpace(os.Getenv("AGENT_NOTIFY_REPLY_ENABLED")); raw != "" {
		cfg.ReplyEnabled = raw == "1" || strings.EqualFold(raw, "true") || strings.EqualFold(raw, "on")
	}
	if raw := strings.TrimSpace(os.Getenv("AGENT_NOTIFY_REPLY_CONFIRMATION")); raw != "" {
		cfg.ReplyConfirmation = raw == "1" || strings.EqualFold(raw, "true") || strings.EqualFold(raw, "on")
	}
	if raw := strings.TrimSpace(os.Getenv("AGENT_NOTIFY_COOLDOWN_MIN")); raw != "" {
		if value, err := strconv.Atoi(raw); err == nil && value > 0 {
			cfg.CooldownMin = value
		}
	}
	if raw := strings.TrimSpace(os.Getenv("AGENT_NOTIFY_THEME")); raw != "" {
		cfg.Theme = strings.ToLower(raw)
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
		// ReplyConfirmation 缺省时为 true（默认开启）；显式 false 才关闭。
		ReplyConfirmation *bool   `json:"replyConfirmation"`
		DefaultAgent      *string `json:"defaultAgent"`
		WidgetAgentMode   *string `json:"widgetAgentMode"`
		Theme             *string `json:"theme"`
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
	if raw.ReplyConfirmation != nil {
		cfg.ReplyConfirmation = *raw.ReplyConfirmation
	}
	if raw.DefaultAgent != nil {
		cfg.DefaultAgent = strings.ToLower(strings.TrimSpace(*raw.DefaultAgent))
	}
	if raw.WidgetAgentMode != nil {
		cfg.WidgetAgentMode = strings.ToLower(strings.TrimSpace(*raw.WidgetAgentMode))
	}
	if raw.Theme != nil {
		cfg.Theme = strings.ToLower(strings.TrimSpace(*raw.Theme))
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
	cfg.DefaultAgent = strings.ToLower(strings.TrimSpace(cfg.DefaultAgent))
	cfg.WidgetAgentMode = strings.ToLower(strings.TrimSpace(cfg.WidgetAgentMode))
	if cfg.WidgetAgentMode != "" && cfg.WidgetAgentMode != "grid" && cfg.WidgetAgentMode != "single" {
		cfg.WidgetAgentMode = ""
	}
	cfg.Theme = strings.ToLower(strings.TrimSpace(cfg.Theme))
	if cfg.Theme != "" && cfg.Theme != "dark" && cfg.Theme != "light" {
		cfg.Theme = ""
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
	tmp, err := os.CreateTemp(filepath.Dir(path), filepath.Base(path)+".*.tmp")
	if err != nil {
		return err
	}
	tmpPath := tmp.Name()
	cleanup := func() {
		_ = tmp.Close()
		_ = os.Remove(tmpPath)
	}

	if err := tmp.Chmod(mode); err != nil {
		cleanup()
		return err
	}
	if _, err := tmp.Write(data); err != nil {
		cleanup()
		return err
	}
	if err := tmp.Close(); err != nil {
		_ = os.Remove(tmpPath)
		return err
	}
	if err := os.Rename(tmpPath, path); err != nil {
		_ = os.Remove(tmpPath)
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
