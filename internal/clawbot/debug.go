package clawbot

import (
	"bytes"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"fmt"
	"os"
	"strings"
	"time"

	"github.com/srafyhucl-cpu/agent-notify/internal/config"
)

const clawbotDebugEnv = "AGENT_NOTIFY_CLAWBOT_DEBUG"

const debugAccountScopeBytes = 16

// DebugOperation identifies one sanitized protocol diagnostic record.
type DebugOperation string

const (
	DebugOperationSendRequest    DebugOperation = "sendmessage-request"
	DebugOperationSendResponse   DebugOperation = "sendmessage"
	DebugOperationSendResult     DebugOperation = "sendmessage-result"
	DebugOperationGetUpdates     DebugOperation = "getupdates"
	DebugOperationGetUpdatesData DebugOperation = "getupdates-result"
)

// DebugSendRequest records the client-generated ID of one outbound send.
type DebugSendRequest struct {
	ClientID     string `json:"client_id"`
	AccountScope string `json:"account_scope,omitempty"`
}

// DebugSendResult records the parsed identifier pair of one outbound send.
type DebugSendResult struct {
	MessageID    string `json:"message_id"`
	ClientID     string `json:"client_id"`
	AccountScope string `json:"account_scope,omitempty"`
}

// DebugInboundReference maps one inbound message to the quoted-message IDs that
// the protocol exposed. Message text is never recorded.
type DebugInboundReference struct {
	MessageID            string   `json:"msg_id"`
	HasReference         bool     `json:"has_reference"`
	ReferencedMessageIDs []string `json:"referenced_msg_ids"`
	AccountScope         string   `json:"account_scope,omitempty"`
	Private              bool     `json:"private"`
	BoundSender          bool     `json:"bound_sender"`
}

// AccountScope returns a non-reversible scope identifier for one bound
// ClawBot account and recipient. It prevents diagnostics from mixing evidence
// across re-login boundaries without persisting raw account identifiers.
func AccountScope(botID, userID string) string {
	raw := strings.TrimSpace(botID) + "\x00" + strings.TrimSpace(userID)
	sum := sha256.Sum256([]byte(raw))
	return hex.EncodeToString(sum[:debugAccountScopeBytes])
}

// writeClawbotDebug records sanitized sendmessage/getupdates responses when
// protocol diagnostics are explicitly enabled. Request payloads are never
// written because they contain the session context token; message text is
// redacted so correlation IDs can be inspected without persisting content.
func writeClawbotDebug(operation DebugOperation, response []byte) {
	if !clawbotDebugEnabled() {
		return
	}
	writeClawbotDebugData(operation, sanitizeClawbotDebugResponse(response))
}

func writeClawbotDebugEvent(operation DebugOperation, value any) {
	if !clawbotDebugEnabled() {
		return
	}
	data, err := json.Marshal(value)
	if err != nil {
		return
	}
	writeClawbotDebugData(operation, sanitizeClawbotDebugResponse(data))
}

func clawbotDebugEnabled() bool {
	return os.Getenv(clawbotDebugEnv) == "1"
}

func writeClawbotDebugData(operation DebugOperation, data []byte) {
	if !clawbotDebugEnabled() {
		return
	}
	paths := config.GetPaths()
	if err := os.MkdirAll(paths.TempDir, 0700); err != nil {
		return
	}
	file, err := os.OpenFile(paths.ClawbotDebugLog, os.O_APPEND|os.O_CREATE|os.O_WRONLY, 0600)
	if err != nil {
		return
	}
	defer file.Close()
	_, _ = fmt.Fprintf(file, "%s %s data=%s\n", time.Now().Format(time.RFC3339), operation, data)
}

func sanitizeClawbotDebugResponse(response []byte) []byte {
	var value any
	decoder := json.NewDecoder(bytes.NewReader(response))
	decoder.UseNumber()
	if err := decoder.Decode(&value); err != nil {
		return []byte("[unparseable response omitted]")
	}
	redactSensitiveValues(value)
	data, err := json.Marshal(value)
	if err != nil {
		return []byte("[response omitted]")
	}
	return data
}

func redactSensitiveValues(value any) {
	switch current := value.(type) {
	case map[string]any:
		for key, child := range current {
			if isSensitiveDebugKey(key) {
				current[key] = "[REDACTED]"
				continue
			}
			redactSensitiveValues(child)
		}
	case []any:
		for _, child := range current {
			redactSensitiveValues(child)
		}
	}
}

func isSensitiveDebugKey(key string) bool {
	key = strings.ToLower(strings.TrimSpace(key))
	if strings.Contains(key, "text") || key == "content" {
		return true
	}
	for _, marker := range []string{"token", "secret", "password", "authorization", "cookie", "credential"} {
		if strings.Contains(key, marker) {
			return true
		}
	}
	return false
}
