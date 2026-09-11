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

	"linkweixin/internal/config"
	"linkweixin/internal/marker"
	"linkweixin/internal/notify"
)

var reWhitespaces = regexp.MustCompile(`\s+`)

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
	for _, m := range matches {
		if fi, err := os.Stat(m); err == nil {
			files = append(files, fileInfo{path: m, modTime: fi.ModTime()})
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

// ConvertCodexArgs parses arguments passed to codex notify.
func ConvertCodexArgs(args []string) (title string, summary string) {
	taskName := ""
	for _, a := range args {
		trimmed := strings.TrimLeft(a, " \t\r\n")
		if !strings.HasPrefix(trimmed, "{") {
			continue
		}

		var evt struct {
			LastAssistantMessage string        `json:"last-assistant-message"`
			InputMessages        []interface{} `json:"input-messages"`
		}
		if err := json.Unmarshal([]byte(trimmed), &evt); err != nil {
			reLast := regexp.MustCompile(`last-assistant-message["':\s]+([^}"]+?)(?:,?\s*input-messages|\s*})`)
			if m := reLast.FindStringSubmatch(trimmed); len(m) == 2 {
				msg := strings.Trim(strings.TrimSpace(m[1]), "\"'")
				if msg != "" {
					runes := []rune(msg)
					if len(runes) > 2000 {
						summary = string(runes[:2000])
					} else {
						summary = msg
					}
				}
			}
			reInput := regexp.MustCompile(`input-messages["':\s]+\["?([^\]"]*)"?\]`)
			if m := reInput.FindStringSubmatch(trimmed); len(m) == 2 {
				firstClean := strings.Trim(strings.TrimSpace(m[1]), "\"'")
				if firstClean != "" {
					firstRunes := []rune(firstClean)
					if len(firstRunes) > 30 {
						taskName = string(firstRunes[:30]) + "…"
					} else {
						taskName = firstClean
					}
				}
			}
			if summary != "" {
				break
			}
			continue
		}

		msg := strings.TrimSpace(evt.LastAssistantMessage)
		if msg != "" {
			runes := []rune(msg)
			if len(runes) > 2000 {
				summary = string(runes[:2000])
			} else {
				summary = msg
			}
		}

		if len(evt.InputMessages) > 0 {
			if first, ok := evt.InputMessages[0].(string); ok {
				firstClean := strings.TrimSpace(reWhitespaces.ReplaceAllString(first, " "))
				if firstClean != "" {
					firstRunes := []rune(firstClean)
					if len(firstRunes) > 30 {
						taskName = string(firstRunes[:30]) + "…"
					} else {
						taskName = firstClean
					}
				}
			}
		}

		if summary != "" {
			break
		}
	}

	if taskName != "" {
		title = fmt.Sprintf("【codex】%s", taskName)
	} else {
		title = "【codex】跑完了"
	}
	return title, summary
}

func writeCodexDebug(line string) {
	if os.Getenv("CODEX_NOTIFY_DEBUG") == "0" {
		return
	}
	paths := config.GetPaths()
	_ = os.MkdirAll(paths.TempDir, 0755)
	entry := fmt.Sprintf("%s %s\n", time.Now().Format(time.RFC3339), line)
	f, err := os.OpenFile(paths.CodexNotifyDebugLog, os.O_APPEND|os.O_CREATE|os.O_WRONLY, 0644)
	if err == nil {
		_, _ = f.WriteString(entry)
		_ = f.Close()
	}
}

// HandleCodex wraps codex turn-ended events: pass-through to computer-use, then push notification.
func HandleCodex(args []string) {
	writeCodexDebug(fmt.Sprintf("args=%s", strings.Join(args, " | ")))

	// Check dry-run and filter flags from arguments passed to cuaExe
	isDry := os.Getenv("CODEX_NOTIFY_DRYRUN") == "1"
	var forwardArgs []string
	for _, a := range args {
		aLower := strings.ToLower(a)
		if aLower == "-dry-run" || aLower == "--dry-run" || aLower == "-dryrun" {
			isDry = true
		} else {
			forwardArgs = append(forwardArgs, a)
		}
	}

	// 1. Read stdin if redirected so we can pass it to computer-use
	stdinBytes := ReadPipedStdinNonBlocking()
	writeCodexDebug(fmt.Sprintf("step 1: stdinBytesLen=%d", len(stdinBytes)))

	// 2. Pass-through to original codex-computer-use.exe
	cuaExe := FindCodexComputerUseExe()
	writeCodexDebug(fmt.Sprintf("step 2: cuaExe=%s", cuaExe))
	if cuaExe != "" {
		cmd := exec.Command(cuaExe, forwardArgs...)
		if len(stdinBytes) > 0 {
			cmd.Stdin = bytes.NewReader(stdinBytes)
		}
		err := cmd.Start()
		writeCodexDebug(fmt.Sprintf("step 2: cuaStart err=%v", err))
		go func() {
			_ = cmd.Wait()
		}()
	}

	// 3. Check marker
	paths := config.GetPaths()
	isOff := marker.IsOff(paths.CodexMarker)
	writeCodexDebug(fmt.Sprintf("step 3: codexMarker=%s isOff=%v", paths.CodexMarker, isOff))
	if isOff {
		writeCodexDebug("marker-off skip push")
		return
	}

	// 4. Parse args & Push
	t0 := time.Now()
	title, summary := ConvertCodexArgs(args)
	writeCodexDebug(fmt.Sprintf("step 4: title=%s summary=%s", title, summary))

	res := notify.SendNotification(notify.NotifyOptions{
		Title:    title,
		Summary:  summary,
		MaxChars: 500,
		DryRun:   isDry,
	})

	secs := time.Since(t0).Seconds()
	writeCodexDebug(fmt.Sprintf("push status=%s secs=%.2f summarylen=%d dry=%v", res.Status, secs, len([]rune(summary)), isDry))

	if isDry && res.DryRunPayload != "" {
		fmt.Println(res.DryRunPayload)
	}
}
