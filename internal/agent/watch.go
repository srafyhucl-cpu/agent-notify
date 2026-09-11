package agent

import (
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"regexp"
	"strings"
	"time"

	"linkweixin/internal/config"
)

var reNotifyDirect = regexp.MustCompile(`(?m)^notify\s*=.*codex-computer-use\.exe`)

// HandleWatch checks codex config and monitors widget process.
func HandleWatch(configPath string, exePath string) {
	paths := config.GetPaths()
	_ = os.MkdirAll(paths.TempDir, 0755)

	if exePath == "" {
		if self, err := os.Executable(); err == nil {
			exePath = self
		}
	}

	// 1. Monitor Widget process
	// If exit marker does not exist and widget is not running, relaunch it
	if _, err := os.Stat(paths.WidgetExitMarker); os.IsNotExist(err) {
		// Check widget-alive.txt timestamp
		needRelaunch := false
		if fi, err := os.Stat(paths.WidgetAliveFile); err != nil {
			needRelaunch = true
		} else if time.Since(fi.ModTime()) > 2*time.Minute {
			needRelaunch = true
		}

		if needRelaunch && exePath != "" {
			cmd := exec.Command(exePath, "widget")
			_ = cmd.Start()
			logLine := fmt.Sprintf("[watch] widget relaunched %s\n", time.Now().Format(time.RFC3339))
			f, err := os.OpenFile(paths.CodexWatchLog, os.O_APPEND|os.O_CREATE|os.O_WRONLY, 0644)
			if err == nil {
				_, _ = f.WriteString(logLine)
				_ = f.Close()
			}
		}
	}

	// 2. Watch codex config.toml
	if configPath == "" {
		if envCfg := os.Getenv("CODEX_CONFIG"); envCfg != "" {
			configPath = envCfg
		} else {
			userProfile := os.Getenv("USERPROFILE")
			configPath = filepath.Join(userProfile, ".codex", "config.toml")
		}
	}

	data, err := os.ReadFile(configPath)
	if err != nil {
		return
	}

	content := string(data)
	contentLower := strings.ToLower(content)
	if strings.Contains(contentLower, "linkweixin") {
		return
	}

	if !reNotifyDirect.MatchString(content) && !strings.Contains(contentLower, "codex-notify.ps1") {
		return
	}

	// Backup
	_ = os.WriteFile(configPath+".bak-notify-wrapper", data, 0644)

	// Replace
	exeSlash := strings.ReplaceAll(exePath, "\\", "/")
	want := fmt.Sprintf(`notify = [ "%s", "codex", "turn-ended" ]`, exeSlash)
	reLine := regexp.MustCompile(`(?m)^notify\s*=.*$`)
	newContent := reLine.ReplaceAllString(content, want)

	if newContent != content {
		_ = os.WriteFile(configPath, []byte(newContent), 0644)
		logLine := fmt.Sprintf("[watch] repatched %s\n", time.Now().Format(time.RFC3339))
		f, err := os.OpenFile(paths.CodexWatchLog, os.O_APPEND|os.O_CREATE|os.O_WRONLY, 0644)
		if err == nil {
			_, _ = f.WriteString(logLine)
			_ = f.Close()
		}
	}
}
