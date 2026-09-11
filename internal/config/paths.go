package config

import (
	"os"
	"path/filepath"
)

// Paths holds runtime paths for linkWeixin.
type Paths struct {
	UserConfigDir        string
	TempDir              string
	AppConfigDir         string
	AppConfigFile        string
	OpenCodeMarker       string
	CodexMarker          string
	AntigravityMarker    string
	PushLog              string
	PluginFile           string
	WidgetErrorLog       string
	WidgetAliveFile      string
	WidgetPosFile        string
	WidgetExitMarker     string
	AntigravityStateFile string
	CodexWatchLog        string
	CodexNotifyDebugLog  string
	AntigravityDebugLog  string
}

// GetPaths resolves runtime paths with environment variable overrides,
// strictly compatible with Get-LinkWeixinPaths in PowerShell.
func GetPaths() Paths {
	userProfile := os.Getenv("USERPROFILE")
	if userProfile == "" {
		if home, err := os.UserHomeDir(); err == nil {
			userProfile = home
		}
	}

	temp := os.Getenv("TEMP")
	if temp == "" {
		temp = os.TempDir()
	}

	configDir := filepath.Join(userProfile, ".config", "opencode")
	tempDir := filepath.Join(temp, "opencode")
	appConfigDir := filepath.Join(userProfile, ".config", "linkweixin")

	openCodeMarker := os.Getenv("OPENCODE_NOTIFY_MARKER_FILE")
	if openCodeMarker == "" {
		openCodeMarker = filepath.Join(configDir, "notify-pushplus.off")
	}

	codexMarker := os.Getenv("CODEX_NOTIFY_MARKER_FILE")
	if codexMarker == "" {
		codexMarker = filepath.Join(configDir, "codex-notify.off")
	}

	antigravityMarker := os.Getenv("ANTIGRAVITY_NOTIFY_MARKER_FILE")
	if antigravityMarker == "" {
		antigravityMarker = filepath.Join(configDir, "antigravity-notify.off")
	}

	pushLog := os.Getenv("OPENCODE_NOTIFY_LOG_FILE")
	if pushLog == "" {
		pushLog = filepath.Join(tempDir, "notify-push.log")
	}

	antigravityStateFile := os.Getenv("ANTIGRAVITY_NOTIFY_STATE_FILE")
	if antigravityStateFile == "" {
		antigravityStateFile = filepath.Join(tempDir, "antigravity-notify-sent.json")
	}

	return Paths{
		UserConfigDir:        configDir,
		TempDir:              tempDir,
		AppConfigDir:         appConfigDir,
		AppConfigFile:        filepath.Join(appConfigDir, "config.json"),
		OpenCodeMarker:       openCodeMarker,
		CodexMarker:          codexMarker,
		AntigravityMarker:    antigravityMarker,
		PushLog:              pushLog,
		PluginFile:           filepath.Join(configDir, "plugins", "notify-pushplus.ts"),
		WidgetErrorLog:       filepath.Join(tempDir, "widget-error.log"),
		WidgetAliveFile:      filepath.Join(tempDir, "widget-alive.txt"),
		WidgetPosFile:        filepath.Join(tempDir, "widget-pos.txt"),
		WidgetExitMarker:     filepath.Join(tempDir, "widget-exit.txt"),
		AntigravityStateFile: antigravityStateFile,
		CodexWatchLog:        filepath.Join(tempDir, "codex-watch.log"),
		CodexNotifyDebugLog:  filepath.Join(tempDir, "codex-notify-debug.log"),
		AntigravityDebugLog:  filepath.Join(tempDir, "antigravity-notify-debug.log"),
	}
}
