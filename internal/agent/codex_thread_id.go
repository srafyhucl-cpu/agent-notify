package agent

import (
	"encoding/json"
	"strings"
)

type codexNotifyEvent struct {
	Type                 string
	LastAssistantMessage string
	InputMessages        []interface{}
	ThreadID             string
	ThreadIDSnake        string
}

func (e *codexNotifyEvent) UnmarshalJSON(data []byte) error {
	var raw struct {
		Type                 string          `json:"type"`
		LastAssistantMessage string          `json:"last-assistant-message"`
		InputMessages        []interface{}   `json:"input-messages"`
		ThreadID             json.RawMessage `json:"thread-id"`
		ThreadIDSnake        json.RawMessage `json:"thread_id"`
	}
	if err := json.Unmarshal(data, &raw); err != nil {
		return err
	}

	e.Type = strings.TrimSpace(raw.Type)
	e.LastAssistantMessage = raw.LastAssistantMessage
	e.InputMessages = raw.InputMessages
	e.ThreadID = parseCodexThreadID(raw.ThreadID)
	e.ThreadIDSnake = parseCodexThreadID(raw.ThreadIDSnake)
	return nil
}

func parseCodexThreadID(raw json.RawMessage) string {
	if len(raw) == 0 || string(raw) == "null" {
		return ""
	}

	decoder := json.NewDecoder(strings.NewReader(string(raw)))
	decoder.UseNumber()
	var value interface{}
	if err := decoder.Decode(&value); err != nil {
		return ""
	}

	switch value := value.(type) {
	case string:
		return strings.TrimSpace(value)
	case json.Number:
		return strings.TrimSpace(value.String())
	default:
		return ""
	}
}

// codexEventThreadID selects the authoritative thread ID of one notify event.
// The hyphen field is Codex's own field name; thread_id is only a compatibility
// alias, so a present hyphen value wins and the alias is never merged with it.
func codexEventThreadID(event codexNotifyEvent) string {
	if value := strings.TrimSpace(event.ThreadID); value != "" {
		return value
	}
	return strings.TrimSpace(event.ThreadIDSnake)
}
