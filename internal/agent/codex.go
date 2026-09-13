package agent

import (
	"bytes"
	"encoding/json"
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"regexp"
	"sort"
	"strings"
	"time"

	"github.com/srafyhucl-cpu/agent-notify/internal/config"
	"github.com/srafyhucl-cpu/agent-notify/internal/marker"
	"github.com/srafyhucl-cpu/agent-notify/internal/notify"
)

var (
	reWhitespaces       = regexp.MustCompile(`\s+`)
	reLastAssistantText = regexp.MustCompile(`last-assistant-message["':\s]+([^}"]+?)(?:,?\s*input-messages|\s*})`)
	reInputMessages     = regexp.MustCompile(`input-messages["':\s]+\["?([^\]"]*)"?\]`)
)

const codexTurnCompleteType = "agent-turn-complete"

// FindCodexComputerUseExe dynamically locates the latest codex-computer-use.exe.
func FindCodexComputerUseExe() string {
	localAppData := os.Getenv("LOCALAPPDATA")
	if localAppData == "" {
		return ""
	}

	pattern := filepath.Join(localAppData, "OpenAI", "Codex", "runtimes", "cua_node", "*", "bin", "node_modules", "@oai", "sky", "bin", "windows", "codex-computer-use.exe")
	matches, err := filepath.Glob(pattern)
	if err != nil || len(matches) == 0 {
		return ""
	}

	type fileInfo struct {
		path    string
		modTime time.Time
	}
	var files []fileInfo
	for _, match := range matches {
		if info, err := os.Stat(match); err == nil {
			files = append(files, fileInfo{path: match, modTime: info.ModTime()})
		}
	}
	if len(files) == 0 {
		return ""
	}
	sort.Slice(files, func(i, j int) bool {
		return files[i].modTime.After(files[j].modTime)
	})
	return files[0].path
}

// ConvertCodexArgs parses one Codex notify payload. Routing identifiers are
// taken only from the same JSON event that supplies notification content; IDs
// from unrelated arguments are never combined.
func ConvertCodexArgs(args []string) (title string, summary string, threadID string) {
	taskName := ""
	contentStarted := false
	contentThreadID := ""
	contentIDMissing := false
	contentIDConflict := false
	fallbackThreadID := ""
	fallbackIDConflict := false

	recordContentThreadID := func(candidate string) {
		if candidate == "" {
			contentIDMissing = true
			return
		}
		if contentThreadID == "" {
			contentThreadID = candidate
			return
		}
		if contentThreadID != candidate {
			contentIDConflict = true
		}
	}
	recordFallbackThreadID := func(candidate string) {
		if candidate == "" {
			return
		}
		if fallbackThreadID == "" {
			fallbackThreadID = candidate
			return
		}
		if fallbackThreadID != candidate {
			fallbackIDConflict = true
		}
	}
	for _, arg := range args {
		trimmed := strings.TrimLeft(arg, " \t\r\n")
		if !strings.HasPrefix(trimmed, "{") {
			continue
		}

		var event codexNotifyEvent
		if err := json.Unmarshal([]byte(trimmed), &event); err != nil {
			foundContent := false
			if summary == "" {
				if match := reLastAssistantText.FindStringSubmatch(trimmed); len(match) == 2 {
					summary = strings.Trim(strings.TrimSpace(match[1]), "\"'")
					foundContent = foundContent || summary != ""
				}
			}
			if taskName == "" {
				if match := reInputMessages.FindStringSubmatch(trimmed); len(match) == 2 {
					taskName = strings.Trim(strings.TrimSpace(match[1]), "\"'")
					foundContent = foundContent || taskName != ""
				}
			}
			if foundContent {
				contentStarted = true
				contentIDMissing = true
			}
			continue
		}

		hasContent := strings.TrimSpace(event.LastAssistantMessage) != "" || len(event.InputMessages) > 0
		isTurnEvent := strings.EqualFold(strings.TrimSpace(event.Type), codexTurnCompleteType)
		eventThreadID := codexEventThreadID(event)
		if hasContent {
			contentStarted = true
			recordContentThreadID(eventThreadID)
		} else if !isTurnEvent {
			recordFallbackThreadID(eventThreadID)
		}

		if message := strings.TrimSpace(event.LastAssistantMessage); message != "" {
			if summary == "" {
				summary = message
			}
		}
		if len(event.InputMessages) > 0 && taskName == "" {
			if first, ok := event.InputMessages[0].(string); ok {
				taskName = strings.TrimSpace(reWhitespaces.ReplaceAllString(first, " "))
			}
		}
	}

	if taskName != "" {
		title = fmt.Sprintf("【codex】%s", taskName)
	} else {
		title = "【codex】跑完了"
	}
	if contentStarted {
		if contentIDMissing || contentIDConflict {
			return title, summary, ""
		}
		return title, summary, contentThreadID
	}
	if fallbackIDConflict {
		return title, summary, ""
	}
	return title, summary, fallbackThreadID
}

func writeCodexDebug(line string) {
	if os.Getenv("AGENT_NOTIFY_CODEX_DEBUG") != "1" {
		return
	}
	paths := config.GetPaths()
	_ = os.MkdirAll(paths.TempDir, 0700)
	entry := fmt.Sprintf("%s %s\n", time.Now().Format(time.RFC3339), line)
	file, err := os.OpenFile(paths.CodexNotifyDebugLog, os.O_APPEND|os.O_CREATE|os.O_WRONLY, 0600)
	if err == nil {
		_, _ = file.WriteString(entry)
		_ = file.Close()
	}
}

// HandleCodex passes the original event through to codex-computer-use and then
// sends an Agent-notify message for the completed turn.
func HandleCodex(args []string) notify.NotifyResult {
	return handleCodex(args, windowsCodexTitleResolver{})
}

func handleCodex(args []string, titles codexTitleResolver) notify.NotifyResult {
	writeCodexDebug(fmt.Sprintf("args=%s", strings.Join(args, " | ")))

	isDry := os.Getenv("AGENT_NOTIFY_CODEX_DRYRUN") == "1"
	var forwardArgs []string
	for _, arg := range args {
		switch strings.ToLower(arg) {
		case "-dry-run", "--dry-run", "-dryrun":
			isDry = true
		default:
			forwardArgs = append(forwardArgs, arg)
		}
	}

	stdinBytes := ReadPipedStdinNonBlocking()
	if cuaExe := FindCodexComputerUseExe(); cuaExe != "" {
		command := exec.Command(cuaExe, forwardArgs...)
		configureHiddenProcess(command)
		if len(stdinBytes) > 0 {
			command.Stdin = bytes.NewReader(stdinBytes)
		}
		if err := command.Start(); err == nil {
			go func() { _ = command.Wait() }()
		}
	}

	paths := config.GetPaths()
	title, summary, threadID := ConvertCodexArgs(args)
	notice := ""
	if threadID != "" {
		resolution := titles.Resolve(threadID, strings.TrimPrefix(title, "【codex】"))
		writeCodexTitleDiagnostic(threadID, resolution)
		if resolution.Name != "" {
			title = "【codex】" + resolution.Name
		}
		notice = resolution.Warning
	}
	opts := notify.NotifyOptions{
		Agent:     "codex",
		SessionID: threadID,
		Title:     title,
		Summary:   summary,
		Notice:    notice,
		MaxChars:  notify.DefaultMaxChars,
		DryRun:    isDry,
	}

	if marker.IsOff(paths.CodexMarker) {
		result := notify.RecordSkipped(opts, "Codex 推送已关闭")
		writeCodexDebug(fmt.Sprintf("push status=%s error=%s", result.Status, result.Error))
		return result
	}
	if !isDry && skippedForQuietHours() {
		result := notify.RecordSkipped(opts, "当前处于勿扰时段")
		writeCodexDebug(fmt.Sprintf("push status=%s error=%s", result.Status, result.Error))
		return result
	}
	if isDoNotDisturbTitle(title) {
		result := notify.RecordSkipped(opts, "标题包含勿扰标记")
		writeCodexDebug(fmt.Sprintf("push status=%s error=%s", result.Status, result.Error))
		return result
	}

	result := notify.SendNotification(opts)
	writeCodexDebug(fmt.Sprintf("push status=%s error=%s", result.Status, result.Error))
	return result
}
