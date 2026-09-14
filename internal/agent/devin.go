package agent

import (
	"github.com/srafyhucl-cpu/agent-notify/internal/config"
	"github.com/srafyhucl-cpu/agent-notify/internal/devinsession"
	"github.com/srafyhucl-cpu/agent-notify/internal/notify"
	"io"
	"strings"
)

const devinDryRunEnv = "AGENT_NOTIFY_DEVIN_DRYRUN"

type devinStopEvent struct {
	SessionID            string `json:"session_id"`
	PromptID             string `json:"prompt_id"`
	HookEventName        string `json:"hook_event_name"`
	StopHookActive       bool   `json:"stop_hook_active"`
	LastAssistantMessage string `json:"last_assistant_message"`
}

// HandleDevinStop processes one Devin Stop hook payload. The CLI wrapper always
// returns an empty JSON object so notification failures never block Devin.
func HandleDevinStop(reader io.Reader) notify.NotifyResult {
	var event devinStopEvent
	if err := readHookJSON(reader, &event); err != nil {
		return notify.NotifyResult{Status: notify.StatusSkipped, Error: err.Error()}
	}
	if event.StopHookActive {
		return notify.NotifyResult{Status: notify.StatusSkipped, Error: "Devin 已在处理 Stop hook"}
	}
	if event.HookEventName != "" && !strings.EqualFold(strings.TrimSpace(event.HookEventName), "Stop") {
		return notify.NotifyResult{Status: notify.StatusSkipped, Error: "Devin hook 事件不是 Stop"}
	}
	event.SessionID = strings.TrimSpace(event.SessionID)
	if event.SessionID == "" {
		return notify.NotifyResult{Status: notify.StatusSkipped, Error: "Devin hook 缺少 session_id"}
	}

	title := ""
	notice := ""
	session, lookupErr := devinsession.LookupSession(event.SessionID)
	if lookupErr != nil {
		notice = "未能读取 Devin 会话标题，已使用默认标题。"
	} else {
		title = session.Title
	}

	return sendWithAgentPolicy(notify.NotifyOptions{
		Agent:     "devin",
		SessionID: event.SessionID,
		Title:     title,
		Summary:   strings.TrimSpace(event.LastAssistantMessage),
		Notice:    notice,
		MaxChars:  notify.DefaultMaxChars,
		DryRun:    envFlag(devinDryRunEnv),
	}, config.GetPaths().DevinMarker, "Devin 推送已关闭")
}
