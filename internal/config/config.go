package config

import (
	"encoding/json"
	"os"
	"path/filepath"
	"regexp"
	"strconv"
	"strings"
	"time"
)

// ChannelPushPlus config for PushPlus WeChat channel.
type ChannelPushPlus struct {
	Enabled bool   `json:"enabled"`
	Token   string `json:"token"`
}

// ChannelWebhook config for Webhook channels (WeCom, Feishu, DingTalk, Custom).
type ChannelWebhook struct {
	Enabled bool   `json:"enabled"`
	Webhook string `json:"webhook"`
}

// ChannelsConfig groups all notification channels.
type ChannelsConfig struct {
	PushPlus ChannelPushPlus `json:"pushplus"`
	WeCom    ChannelWebhook  `json:"wecom"`
	Feishu   ChannelWebhook  `json:"feishu"`
	DingTalk ChannelWebhook  `json:"dingtalk"`
	Custom   ChannelWebhook  `json:"custom"`
}

// AppConfig represents ~/.config/linkweixin/config.json content.
type AppConfig struct {
	Channels    ChannelsConfig `json:"channels"`
	QuietHours  string         `json:"quietHours"`
	CooldownMin int            `json:"cooldownMin"`
}

// DefaultConfig builds default configuration merged with current environment variables.
func DefaultConfig() AppConfig {
	pushToken := os.Getenv("PUSHPLUS_TOKEN")

	wecomHook := os.Getenv("WECOM_WEBHOOK_URL")
	feishuHook := os.Getenv("FEISHU_WEBHOOK_URL")
	dingHook := os.Getenv("DINGTALK_WEBHOOK_URL")
	customHook := os.Getenv("LINKWEIXIN_WEBHOOK_URL")

	quiet := os.Getenv("OPENCODE_NOTIFY_QUIET")
	if quiet == "" {
		quiet = os.Getenv("ANTIGRAVITY_NOTIFY_QUIET")
	}

	cooldown := 10
	if cdStr := os.Getenv("OPENCODE_NOTIFY_COOLDOWN_MIN"); cdStr != "" {
		if n, err := strconv.Atoi(strings.TrimSpace(cdStr)); err == nil && n > 0 {
			cooldown = n
		}
	} else if cdStr := os.Getenv("ANTIGRAVITY_NOTIFY_COOLDOWN_MIN"); cdStr != "" {
		if n, err := strconv.Atoi(strings.TrimSpace(cdStr)); err == nil && n > 0 {
			cooldown = n
		}
	}

	return AppConfig{
		Channels: ChannelsConfig{
			PushPlus: ChannelPushPlus{
				Enabled: true,
				Token:   pushToken,
			},
			WeCom: ChannelWebhook{
				Enabled: wecomHook != "",
				Webhook: wecomHook,
			},
			Feishu: ChannelWebhook{
				Enabled: feishuHook != "",
				Webhook: feishuHook,
			},
			DingTalk: ChannelWebhook{
				Enabled: dingHook != "",
				Webhook: dingHook,
			},
			Custom: ChannelWebhook{
				Enabled: customHook != "",
				Webhook: customHook,
			},
		},
		QuietHours:  quiet,
		CooldownMin: cooldown,
	}
}

// LoadConfig reads config.json or falls back to defaults merged with env vars.
func LoadConfig(configPath string) AppConfig {
	cfg := DefaultConfig()
	if configPath == "" {
		paths := GetPaths()
		configPath = paths.AppConfigFile
	}

	data, err := os.ReadFile(configPath)
	if err != nil {
		return cfg
	}

	// Partially parse raw JSON to selectively override fields
	var raw struct {
		Channels *struct {
			PushPlus *struct {
				Enabled *bool   `json:"enabled"`
				Token   *string `json:"token"`
			} `json:"pushplus"`
			WeCom *struct {
				Enabled *bool   `json:"enabled"`
				Webhook *string `json:"webhook"`
			} `json:"wecom"`
			Feishu *struct {
				Enabled *bool   `json:"enabled"`
				Webhook *string `json:"webhook"`
			} `json:"feishu"`
			DingTalk *struct {
				Enabled *bool   `json:"enabled"`
				Webhook *string `json:"webhook"`
			} `json:"dingtalk"`
			Custom *struct {
				Enabled *bool   `json:"enabled"`
				Webhook *string `json:"webhook"`
			} `json:"custom"`
		} `json:"channels"`
		QuietHours  *string `json:"quietHours"`
		CooldownMin *int    `json:"cooldownMin"`
	}

	if err := json.Unmarshal(data, &raw); err != nil {
		return cfg
	}

	if raw.Channels != nil {
		if raw.Channels.PushPlus != nil {
			if raw.Channels.PushPlus.Enabled != nil {
				cfg.Channels.PushPlus.Enabled = *raw.Channels.PushPlus.Enabled
			}
			if raw.Channels.PushPlus.Token != nil && strings.TrimSpace(*raw.Channels.PushPlus.Token) != "" {
				cfg.Channels.PushPlus.Token = *raw.Channels.PushPlus.Token
			}
		}
		if raw.Channels.WeCom != nil {
			if raw.Channels.WeCom.Enabled != nil {
				cfg.Channels.WeCom.Enabled = *raw.Channels.WeCom.Enabled
			}
			if raw.Channels.WeCom.Webhook != nil && strings.TrimSpace(*raw.Channels.WeCom.Webhook) != "" {
				cfg.Channels.WeCom.Webhook = *raw.Channels.WeCom.Webhook
			}
		}
		if raw.Channels.Feishu != nil {
			if raw.Channels.Feishu.Enabled != nil {
				cfg.Channels.Feishu.Enabled = *raw.Channels.Feishu.Enabled
			}
			if raw.Channels.Feishu.Webhook != nil && strings.TrimSpace(*raw.Channels.Feishu.Webhook) != "" {
				cfg.Channels.Feishu.Webhook = *raw.Channels.Feishu.Webhook
			}
		}
		if raw.Channels.DingTalk != nil {
			if raw.Channels.DingTalk.Enabled != nil {
				cfg.Channels.DingTalk.Enabled = *raw.Channels.DingTalk.Enabled
			}
			if raw.Channels.DingTalk.Webhook != nil && strings.TrimSpace(*raw.Channels.DingTalk.Webhook) != "" {
				cfg.Channels.DingTalk.Webhook = *raw.Channels.DingTalk.Webhook
			}
		}
		if raw.Channels.Custom != nil {
			if raw.Channels.Custom.Enabled != nil {
				cfg.Channels.Custom.Enabled = *raw.Channels.Custom.Enabled
			}
			if raw.Channels.Custom.Webhook != nil && strings.TrimSpace(*raw.Channels.Custom.Webhook) != "" {
				cfg.Channels.Custom.Webhook = *raw.Channels.Custom.Webhook
			}
		}
	}

	if raw.QuietHours != nil {
		cfg.QuietHours = *raw.QuietHours
	}
	if raw.CooldownMin != nil && *raw.CooldownMin > 0 {
		cfg.CooldownMin = *raw.CooldownMin
	}

	return cfg
}

// SaveConfig writes AppConfig to config.json.
func SaveConfig(cfg AppConfig, configPath string) error {
	if configPath == "" {
		paths := GetPaths()
		configPath = paths.AppConfigFile
	}

	dir := filepath.Dir(configPath)
	if err := os.MkdirAll(dir, 0755); err != nil {
		return err
	}

	data, err := json.MarshalIndent(cfg, "", "  ")
	if err != nil {
		return err
	}

	return os.WriteFile(configPath, data, 0644)
}

var quietRegex = regexp.MustCompile(`^\s*(\d{1,2})\s*-\s*(\d{1,2})\s*$`)

// IsInQuietHours returns true if current time falls within the configured quiet hours (e.g. "23-8").
func IsInQuietHours(quietRaw string, t time.Time) bool {
	if strings.TrimSpace(quietRaw) == "" {
		return false
	}
	m := quietRegex.FindStringSubmatch(quietRaw)
	if len(m) != 3 {
		return false
	}
	s, err1 := strconv.Atoi(m[1])
	e, err2 := strconv.Atoi(m[2])
	if err1 != nil || err2 != nil {
		return false
	}
	if s < 0 || s > 23 || e < 0 || e > 23 || s == e {
		return false
	}
	h := t.Hour()
	if s < e {
		return h >= s && h < e
	}
	return h >= s || h < e
}
