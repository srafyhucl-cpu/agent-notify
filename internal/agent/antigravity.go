package agent

import (
	"bufio"
	"encoding/json"
	"fmt"
	"os"
	"path/filepath"
	"regexp"
	"strings"
	"time"

	"linkweixin/internal/config"
	"linkweixin/internal/marker"
	"linkweixin/internal/notify"
)

var (
	reUserRequest = regexp.MustCompile(`(?si)<USER_REQUEST>(.*?)</USER_REQUEST>`)
	reTags        = regexp.MustCompile(`(?s)<[^>]+>`)
	reTaskHeader  = regexp.MustCompile(`(?i)(?:\*{1,2})?(?:Task|任务)(?:\*{1,2})?[:：]\s*(.+)`)
)

func writeAntigravityDebug(line string) {
	if os.Getenv("ANTIGRAVITY_NOTIFY_DEBUG") == "0" {
		return
	}
	paths := config.GetPaths()
	_ = os.MkdirAll(paths.TempDir, 0755)
	entry := fmt.Sprintf("%s %s\n", time.Now().Format(time.RFC3339), line)
	f, err := os.OpenFile(paths.AntigravityDebugLog, os.O_APPEND|os.O_CREATE|os.O_WRONLY, 0644)
	if err == nil {
		_, _ = f.WriteString(entry)
		_ = f.Close()
	}
}

func getTranscriptLineText(obj map[string]interface{}) string {
	if obj == nil {
		return ""
	}

	// Helper to extract string from interface
	var extract func(v interface{}) string
	extract = func(v interface{}) string {
		if v == nil {
			return ""
		}
		if s, ok := v.(string); ok {
			return s
		}
		if list, ok := v.([]interface{}); ok {
			var parts []string
			for _, item := range list {
				if s, ok := item.(string); ok {
					parts = append(parts, s)
				} else if m, ok := item.(map[string]interface{}); ok {
					if t, ok := m["text"].(string); ok && t != "" {
						parts = append(parts, t)
					}
				}
			}
			if len(parts) > 0 {
				return strings.Join(parts, "\n")
			}
		}
		if m, ok := v.(map[string]interface{}); ok {
			if t, ok := m["text"].(string); ok {
				return t
			}
			if c, ok := m["content"]; ok {
				return extract(c)
			}
			if p, ok := m["parts"]; ok {
				return extract(p)
			}
		}
		return ""
	}

	if c, ok := obj["content"]; ok && c != nil {
		if res := extract(c); res != "" {
			return res
		}
	}
	if msg, ok := obj["message"].(map[string]interface{}); ok && msg != nil {
		if res := extract(msg); res != "" {
			return res
		}
	}
	if parts, ok := obj["parts"]; ok && parts != nil {
		if res := extract(parts); res != "" {
			return res
		}
	}
	for _, key := range []string{"text", "prompt", "response"} {
		if s, ok := obj[key].(string); ok && s != "" {
			return s
		}
	}

	return ""
}

func isAssistantTurn(obj map[string]interface{}) bool {
	if obj == nil {
		return false
	}
	var role string
	for _, key := range []string{"role", "type", "sender", "author"} {
		if r, ok := obj[key].(string); ok && r != "" {
			role = strings.ToLower(strings.TrimSpace(r))
			break
		}
	}
	if role == "" {
		if msg, ok := obj["message"].(map[string]interface{}); ok && msg != nil {
			if r, ok := msg["role"].(string); ok {
				role = strings.ToLower(strings.TrimSpace(r))
			}
		}
	}
	return role == "assistant" || role == "model" || role == "agent"
}

// HandleAntigravity processes Stop hook events from Google Antigravity.
func HandleAntigravity(payloadJson string, dryRun bool) {
	defer func() {
		if r := recover(); r != nil {
			writeAntigravityDebug(fmt.Sprintf("recovered from panic: %v", r))
			fmt.Println("{}")
		}
	}()

	raw := payloadJson
	if strings.TrimSpace(raw) == "" {
		if data := ReadPipedStdinNonBlocking(); len(data) > 0 {
			raw = DecodeConsoleBytes(data)
		}
	}

	writeAntigravityDebug(fmt.Sprintf("enter: rawLen=%d", len(raw)))
	if strings.TrimSpace(raw) == "" {
		writeAntigravityDebug("skip: empty payload")
		fmt.Println("{}")
		return
	}

	var hookContext struct {
		FullyIdle      interface{} `json:"fullyIdle"`
		TranscriptPath string      `json:"transcriptPath"`
		ConversationID string      `json:"conversationId"`
		SessionId      string      `json:"sessionId"`
		SessionIDUpper string      `json:"sessionID"`
	}

	if err := json.Unmarshal([]byte(raw), &hookContext); err != nil {
		writeAntigravityDebug(fmt.Sprintf("bad json: %v", err))
		fmt.Println("{}")
		return
	}

	// Guard 1: fullyIdle must be true
	isFullyIdle := false
	switch v := hookContext.FullyIdle.(type) {
	case bool:
		isFullyIdle = v
	case string:
		vLower := strings.ToLower(strings.TrimSpace(v))
		isFullyIdle = vLower == "true" || vLower == "1"
	case float64:
		isFullyIdle = v == 1
	}

	if !isFullyIdle {
		writeAntigravityDebug(fmt.Sprintf("skip: fullyIdle is not true (val=%v)", hookContext.FullyIdle))
		fmt.Println("{}")
		return
	}

	// Guard 2: marker check
	paths := config.GetPaths()
	if marker.IsOff(paths.AntigravityMarker) {
		writeAntigravityDebug(fmt.Sprintf("skip: marker-off (%s)", paths.AntigravityMarker))
		fmt.Println("{}")
		return
	}

	// Guard 3: quiet hours
	cfg := config.LoadConfig("")
	quiet := cfg.QuietHours
	if quiet == "" {
		quiet = os.Getenv("ANTIGRAVITY_NOTIFY_QUIET")
	}
	if quiet == "" {
		quiet = os.Getenv("OPENCODE_NOTIFY_QUIET")
	}
	if config.IsInQuietHours(quiet, time.Now()) {
		writeAntigravityDebug(fmt.Sprintf("skip: in quiet hours (%s)", quiet))
		fmt.Println("{}")
		return
	}

	// Guard 4: cooldown
	sessionID := hookContext.ConversationID
	if sessionID == "" {
		sessionID = hookContext.SessionId
	}
	if sessionID == "" {
		sessionID = hookContext.SessionIDUpper
	}
	if sessionID == "" && hookContext.TranscriptPath != "" {
		sessionID = filepath.Base(hookContext.TranscriptPath)
	}
	if sessionID == "" {
		sessionID = "default-antigravity"
	}

	cooldownMin := cfg.CooldownMin
	if cooldownMin <= 0 {
		cooldownMin = 10
	}

	isDry := dryRun || os.Getenv("ANTIGRAVITY_NOTIFY_DRYRUN") == "1"
	if !isDry {
		stateFile := paths.AntigravityStateFile
		sentMap := make(map[string]float64)
		if data, err := os.ReadFile(stateFile); err == nil {
			_ = json.Unmarshal(data, &sentMap)
		}

		nowEpoch := float64(time.Now().UnixMilli())
		cooldownMs := float64(cooldownMin * 60 * 1000)

		if lastSent, exists := sentMap[sessionID]; exists {
			if (nowEpoch - lastSent) < cooldownMs {
				writeAntigravityDebug(fmt.Sprintf("skip: cooldown sid=%s", sessionID))
				fmt.Println("{}")
				return
			}
		}

		sentMap[sessionID] = nowEpoch
		// Clean expired entries
		for k, v := range sentMap {
			if (nowEpoch - v) > cooldownMs {
				delete(sentMap, k)
			}
		}
		_ = os.MkdirAll(filepath.Dir(stateFile), 0755)
		if data, err := json.Marshal(sentMap); err == nil {
			_ = os.WriteFile(stateFile, data, 0644)
		}
	}

	// Extract Title & Summary from transcriptPath
	taskTitle := "任务完成"
	modelSummary := ""

	if hookContext.TranscriptPath != "" {
		if file, err := os.Open(hookContext.TranscriptPath); err == nil {
			var allLines []string
			reader := bufio.NewReader(file)
			for {
				line, err := reader.ReadString('\n')
				trimmed := strings.TrimSpace(line)
				if trimmed != "" {
					allLines = append(allLines, trimmed)
				}
				if err != nil {
					break
				}
			}
			_ = file.Close()

			// First line: task title
			if len(allLines) > 0 {
				var firstObj map[string]interface{}
				_ = json.Unmarshal([]byte(allLines[0]), &firstObj)
				userText := getTranscriptLineText(firstObj)
				if userText == "" {
					userText = allLines[0]
				}

				if m := reUserRequest.FindStringSubmatch(userText); len(m) == 2 {
					userText = m[1]
				}
				userText = reTags.ReplaceAllString(userText, " ")
				userText = strings.TrimSpace(reWhitespaces.ReplaceAllString(userText, " "))

				if tm := reTaskHeader.FindStringSubmatch(userText); len(tm) == 2 {
					userText = strings.TrimSpace(strings.Trim(tm[1], "*"))
				}
				runes := []rune(userText)
				if len(runes) > 30 {
					userText = string(runes[:30]) + "…"
				}
				if userText != "" {
					taskTitle = userText
				}
			}

			// Tail lines: assistant response summary
			tailStart := len(allLines) - 100
			if tailStart < 0 {
				tailStart = 0
			}
			tailLines := allLines[tailStart:]

			for i := len(tailLines) - 1; i >= 0; i-- {
				l := tailLines[i]
				var obj map[string]interface{}
				if err := json.Unmarshal([]byte(l), &obj); err == nil && isAssistantTurn(obj) {
					text := getTranscriptLineText(obj)
					trimmed := strings.TrimSpace(text)
					if trimmed != "" {
						runes := []rune(trimmed)
						if len(runes) > 2000 {
							modelSummary = string(runes[:2000])
						} else {
							modelSummary = trimmed
						}
						break
					}
				}
			}

			// Fallback if no role matched
			if modelSummary == "" && len(tailLines) > 1 {
				for i := len(tailLines) - 1; i >= 1; i-- {
					cand := tailLines[i]
					var candObj map[string]interface{}
					_ = json.Unmarshal([]byte(cand), &candObj)
					candText := getTranscriptLineText(candObj)
					if candText == "" {
						candText = cand
					}
					candText = strings.TrimSpace(candText)
					if candText != "" {
						runes := []rune(candText)
						if len(runes) > 2000 {
							modelSummary = string(runes[:2000])
						} else {
							modelSummary = candText
						}
						break
					}
				}
			}
		} else {
			writeAntigravityDebug(fmt.Sprintf("open transcriptPath error: %v", err))
		}
	}

	title := fmt.Sprintf("【Antigravity】%s", taskTitle)
	res := notify.SendNotification(notify.NotifyOptions{
		Title:    title,
		Summary:  modelSummary,
		MaxChars: 1000,
		DryRun:   isDry,
	})

	writeAntigravityDebug(fmt.Sprintf("push completed: title=%s summarylen=%d dry=%v", title, len([]rune(modelSummary)), isDry))

	if isDry && res.DryRunPayload != "" {
		fmt.Println(res.DryRunPayload)
	} else {
		fmt.Println("{}")
	}
}
