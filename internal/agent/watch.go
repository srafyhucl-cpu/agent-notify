package agent

import (
	"fmt"
	"os"
	"path/filepath"
	"regexp"
	"strings"
	"time"

	"github.com/srafyhucl-cpu/agent-notify/internal/config"
)

var (
	reDirectCodexNotify = regexp.MustCompile(`(?m)^notify\s*=.*codex-computer-use\.exe`)
	reAnyNotifyLine     = regexp.MustCompile(`(?m)^notify\s*=.*$`)
)

// HandleWatch restores the Codex notify line only when it still points directly
// at codex-computer-use.exe. Custom notify commands are never overwritten.
func HandleWatch(configPath string, exePath string) error {
	if strings.TrimSpace(configPath) == "" {
		configPath = strings.TrimSpace(os.Getenv("AGENT_NOTIFY_CODEX_CONFIG"))
	}
	if configPath == "" {
		home := strings.TrimSpace(os.Getenv("USERPROFILE"))
		if home == "" {
			if value, err := os.UserHomeDir(); err == nil {
				home = value
			}
		}
		configPath = filepath.Join(home, ".codex", "config.toml")
	}

	if strings.TrimSpace(exePath) == "" {
		self, err := os.Executable()
		if err != nil {
			return fmt.Errorf("locate agent-notify executable: %w", err)
		}
		exePath = self
	}

	data, err := os.ReadFile(configPath)
	if err != nil {
		return err
	}
	content := string(data)
	notifyLine := reAnyNotifyLine.FindString(content)
	if notifyLine == "" ||
		strings.Contains(strings.ToLower(notifyLine), "agent-notify") ||
		!reDirectCodexNotify.MatchString(notifyLine) {
		return nil
	}

	backupPath := configPath + ".bak-notify-wrapper"
	if err := os.WriteFile(backupPath, data, 0600); err != nil {
		return fmt.Errorf("write config backup: %w", err)
	}

	exeSlash := strings.ReplaceAll(exePath, "\\", "/")
	replacement := fmt.Sprintf(`notify = [ "%s", "codex", "turn-ended" ]`, exeSlash)
	// 用字面量替换：replacement 内嵌了安装路径，若走正则展开，路径中的 `$`
	// 会被当成分组引用而被吞掉（例如 C:\Tools\$weird\...）。
	updated := reAnyNotifyLine.ReplaceAllLiteralString(content, replacement)
	if updated == content {
		return nil
	}
	if err := os.WriteFile(configPath, []byte(updated), 0600); err != nil {
		return fmt.Errorf("write codex config: %w", err)
	}

	paths := config.GetPaths()
	_ = os.MkdirAll(paths.TempDir, 0700)
	entry := fmt.Sprintf("[watch] repatched %s at %s\n", configPath, time.Now().Format(time.RFC3339))
	file, err := os.OpenFile(paths.CodexWatchLog, os.O_APPEND|os.O_CREATE|os.O_WRONLY, 0600)
	if err == nil {
		_, _ = file.WriteString(entry)
		_ = file.Close()
	}
	return nil
}
