package clawbot

import (
	"bufio"
	"encoding/json"
	"fmt"
	"os"
	"strings"
	"time"
)

const (
	initialDebugLogScanBuffer = 64 * 1024
	maxDebugLogLineBytes      = 2 * 1024 * 1024
	debugLogDataSeparator     = " data="
	debugLogHeaderFields      = 2
)

// DebugEntry is one parsed line of the sanitized protocol diagnostics log.
type DebugEntry struct {
	Timestamp time.Time
	Operation DebugOperation
	Data      json.RawMessage
}

// ReadClawbotDebugLog returns every parseable entry of the protocol diagnostics
// log. Corrupt or partially written lines are skipped so a single bad line
// cannot hide the surrounding evidence.
func ReadClawbotDebugLog(path string) ([]DebugEntry, error) {
	file, err := os.Open(path)
	if err != nil {
		return nil, err
	}
	defer file.Close()

	var entries []DebugEntry
	scanner := bufio.NewScanner(file)
	scanner.Buffer(make([]byte, initialDebugLogScanBuffer), maxDebugLogLineBytes)
	for scanner.Scan() {
		if entry, ok := parseDebugLogLine(scanner.Text()); ok {
			entries = append(entries, entry)
		}
	}
	if err := scanner.Err(); err != nil {
		return nil, fmt.Errorf("clawbot: read debug log: %w", err)
	}
	return entries, nil
}

func parseDebugLogLine(line string) (DebugEntry, bool) {
	line = strings.TrimSpace(line)
	separator := strings.Index(line, debugLogDataSeparator)
	if separator < 0 {
		return DebugEntry{}, false
	}
	header := strings.Fields(line[:separator])
	if len(header) != debugLogHeaderFields {
		return DebugEntry{}, false
	}
	timestamp, err := time.Parse(time.RFC3339, header[0])
	if err != nil {
		return DebugEntry{}, false
	}
	data := strings.TrimSpace(line[separator+len(debugLogDataSeparator):])
	if data == "" || !json.Valid([]byte(data)) {
		return DebugEntry{}, false
	}
	return DebugEntry{
		Timestamp: timestamp,
		Operation: DebugOperation(header[1]),
		Data:      json.RawMessage(data),
	}, true
}

// SendRequest decodes a sendmessage-request entry.
func (e DebugEntry) SendRequest() (DebugSendRequest, bool) {
	if e.Operation != DebugOperationSendRequest {
		return DebugSendRequest{}, false
	}
	var value DebugSendRequest
	if err := json.Unmarshal(e.Data, &value); err != nil {
		return DebugSendRequest{}, false
	}
	value.ClientID = strings.TrimSpace(value.ClientID)
	return value, value.ClientID != ""
}

// SendResult decodes a sendmessage-result entry.
func (e DebugEntry) SendResult() (DebugSendResult, bool) {
	if e.Operation != DebugOperationSendResult {
		return DebugSendResult{}, false
	}
	var value DebugSendResult
	if err := json.Unmarshal(e.Data, &value); err != nil {
		return DebugSendResult{}, false
	}
	value.MessageID = strings.TrimSpace(value.MessageID)
	value.ClientID = strings.TrimSpace(value.ClientID)
	return value, value.MessageID != "" || value.ClientID != ""
}

// InboundReferences decodes a getupdates-result entry. Records without a quoted
// message keep empty ID lists so callers can still count observed messages.
func (e DebugEntry) InboundReferences() ([]DebugInboundReference, bool) {
	if e.Operation != DebugOperationGetUpdatesData {
		return nil, false
	}
	var value []DebugInboundReference
	if err := json.Unmarshal(e.Data, &value); err != nil {
		return nil, false
	}
	for index := range value {
		value[index].MessageID = strings.TrimSpace(value[index].MessageID)
		value[index].ReferencedMessageIDs = normalizeDebugIDs(value[index].ReferencedMessageIDs)
	}
	return value, true
}

func normalizeDebugIDs(ids []string) []string {
	normalized := make([]string, 0, len(ids))
	seen := make(map[string]struct{}, len(ids))
	for _, id := range ids {
		id = strings.TrimSpace(id)
		if id == "" {
			continue
		}
		if _, duplicate := seen[id]; duplicate {
			continue
		}
		seen[id] = struct{}{}
		normalized = append(normalized, id)
	}
	return normalized
}
