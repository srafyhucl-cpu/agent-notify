package agent

import (
	"bytes"
	"encoding/json"
	"io"
	"strings"

	"github.com/srafyhucl-cpu/agent-notify/internal/config"
	"github.com/srafyhucl-cpu/agent-notify/internal/notify"
)

const antigravityDryRunEnv = "AGENT_NOTIFY_ANTIGRAVITY_DRYRUN"

type antigravityStopEvent struct {
	ConversationID string          `json:"conversationId"`
	TranscriptPath string          `json:"transcriptPath"`
	FullyIdle      *bool           `json:"fullyIdle"`
	Error          json.RawMessage `json:"error"`
}

// HandleAntigravityStop processes one Antigravity Stop hook payload. The CLI
// wrapper always returns an empty JSON object so notification failures never
// block the agent.
func HandleAntigravityStop(reader io.Reader) notify.NotifyResult {
	var event antigravityStopEvent
	if err := readHookJSON(reader, &event); err != nil {
		return notify.NotifyResult{Status: notify.StatusSkipped, Error: err.Error()}
	}
	if event.FullyIdle == nil || !*event.FullyIdle {
		return notify.NotifyResult{Status: notify.StatusSkipped, Error: "Antigravity 尚未完全空闲"}
	}
	event.ConversationID = strings.TrimSpace(event.ConversationID)
	if event.ConversationID == "" {
		return notify.NotifyResult{Status: notify.StatusSkipped, Error: "Antigravity hook 缺少 conversationId"}
	}

	notice := ""
	if hasJSONValue(event.Error) {
		notice = "Antigravity 结束时报告了错误。"
	}
	title := resolveAntigravityTitle(
		config.GetPaths().AntigravityAnnotations,
		event.ConversationID,
		event.TranscriptPath,
	)
	if title.Warning != "" {
		if notice != "" {
			notice += "\n"
		}
		notice += title.Warning
	}
	return sendWithAgentPolicy(notify.NotifyOptions{
		Agent:     "antigravity",
		SessionID: event.ConversationID,
		Title:     title.Title,
		Summary:   readAntigravityTranscriptSummary(event.TranscriptPath),
		Notice:    notice,
		DryRun:    envFlag(antigravityDryRunEnv),
	}, config.GetPaths().AntigravityMarker, "Antigravity 推送已关闭")
}

func hasJSONValue(raw json.RawMessage) bool {
	value := bytes.TrimSpace(raw)
	if len(value) == 0 {
		return false
	}
	var decoded any
	if err := json.Unmarshal(value, &decoded); err != nil {
		return true
	}
	switch typed := decoded.(type) {
	case nil:
		return false
	case string:
		return strings.TrimSpace(typed) != ""
	case []any:
		return len(typed) > 0
	case map[string]any:
		return len(typed) > 0
	default:
		return true
	}
}
