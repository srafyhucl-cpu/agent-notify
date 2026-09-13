package config

import (
	"os"
	"path/filepath"
	"strings"
)

type Paths struct {
	ConfigDir           string
	ConfigFile          string
	CredentialFile      string
	TempDir             string
	OpenCodeMarker      string
	CodexMarker         string
	PushLog             string
	PluginFile          string
	WidgetErrorLog      string
	WidgetAliveFile     string
	WidgetPosFile       string
	WidgetExitMarker    string
	CodexWatchLog       string
	CodexNotifyDebugLog string
	CodexTitleLog       string
	WidgetTraceLog      string
	BootLog             string
	ReplyRouteFile      string
	ReplyStateFile      string
	OpenCodeReplyDir    string
	ClawbotDebugLog     string
	ReplyDebugLog       string
}

// GetPaths resolves all Agent-notify runtime paths. Every location can be
// overridden to keep tests and portable installs away from the user profile.
func GetPaths() Paths {
	home := strings.TrimSpace(os.Getenv("USERPROFILE"))
	if home == "" {
		if value, err := os.UserHomeDir(); err == nil {
			home = value
		}
	}

	configDir := strings.TrimSpace(os.Getenv("AGENT_NOTIFY_CONFIG_DIR"))
	if configDir == "" {
		configDir = filepath.Join(home, ".config", "agent-notify")
	}

	tempRoot := strings.TrimSpace(os.Getenv("TEMP"))
	if tempRoot == "" {
		tempRoot = os.TempDir()
	}
	tempDir := strings.TrimSpace(os.Getenv("AGENT_NOTIFY_TEMP_DIR"))
	if tempDir == "" {
		tempDir = filepath.Join(tempRoot, "agent-notify")
	}

	configFile := envOr("AGENT_NOTIFY_CONFIG_FILE", filepath.Join(configDir, "config.json"))
	credentialFile := envOr("AGENT_NOTIFY_CREDENTIAL_FILE", filepath.Join(configDir, "clawbot.json"))
	openCodeMarker := envOr("AGENT_NOTIFY_OPENCODE_MARKER_FILE", filepath.Join(configDir, "opencode.off"))
	codexMarker := envOr("AGENT_NOTIFY_CODEX_MARKER_FILE", filepath.Join(configDir, "codex.off"))
	pushLog := envOr("AGENT_NOTIFY_LOG_FILE", filepath.Join(tempDir, "push.log"))
	pluginFile := envOr("AGENT_NOTIFY_PLUGIN_FILE", filepath.Join(home, ".config", "opencode", "plugins", "agent-notify.ts"))
	replyRouteFile := envOr("AGENT_NOTIFY_REPLY_ROUTE_FILE", filepath.Join(configDir, "reply-routes.jsonl"))
	replyStateFile := envOr("AGENT_NOTIFY_REPLY_STATE_FILE", filepath.Join(configDir, "reply-state.jsonl"))
	openCodeReplyDir := envOr("AGENT_NOTIFY_OPENCODE_REPLY_DIR", filepath.Join(configDir, "opencode-reply-inbox"))

	return Paths{
		ConfigDir:           configDir,
		ConfigFile:          configFile,
		CredentialFile:      credentialFile,
		TempDir:             tempDir,
		OpenCodeMarker:      openCodeMarker,
		CodexMarker:         codexMarker,
		PushLog:             pushLog,
		PluginFile:          pluginFile,
		ReplyRouteFile:      replyRouteFile,
		ReplyStateFile:      replyStateFile,
		OpenCodeReplyDir:    openCodeReplyDir,
		WidgetErrorLog:      filepath.Join(tempDir, "widget-error.log"),
		WidgetAliveFile:     filepath.Join(tempDir, "widget-alive.txt"),
		WidgetPosFile:       filepath.Join(tempDir, "widget-pos.txt"),
		WidgetExitMarker:    filepath.Join(tempDir, "widget-exit.txt"),
		CodexWatchLog:       filepath.Join(tempDir, "codex-watch.log"),
		CodexNotifyDebugLog: filepath.Join(tempDir, "codex-notify-debug.log"),
		CodexTitleLog:       filepath.Join(tempDir, "codex-title.log"),
		ClawbotDebugLog:     filepath.Join(tempDir, "clawbot-debug.log"),
		ReplyDebugLog:       filepath.Join(tempDir, "reply-debug.log"),
		WidgetTraceLog:      filepath.Join(tempDir, "widget-trace.log"),
		BootLog:             filepath.Join(tempDir, "boot.log"),
	}
}

func envOr(key, fallback string) string {
	if value := strings.TrimSpace(os.Getenv(key)); value != "" {
		return value
	}
	return fallback
}
