package clawbot

import (
	"encoding/json"
	"errors"
	"io/fs"
	"os"
	"path/filepath"
	"testing"
)

func TestReadClawbotDebugLogSkipsCorruptLines(t *testing.T) {
	path := filepath.Join(t.TempDir(), "clawbot-debug.log")
	content := "" +
		"2026-09-13T06:30:10+08:00 " + string(DebugOperationSendResult) + " data={\"message_id\":\"platform-1\",\"client_id\":\"client-1\"}\n" +
		"not-a-log-line\n" +
		"2026-09-13T06:30:11+08:00 " + string(DebugOperationGetUpdatesData) + " data=not-json\n" +
		"2026-09-13T06:30:12+08:00 " + string(DebugOperationGetUpdatesData) + " data=[{\"msg_id\":\"reply-1\",\"referenced_msg_ids\":[\"platform-1\"]}]\n"
	if err := os.WriteFile(path, []byte(content), 0o600); err != nil {
		t.Fatalf("WriteFile: %v", err)
	}

	entries, err := ReadClawbotDebugLog(path)
	if err != nil {
		t.Fatalf("ReadClawbotDebugLog: %v", err)
	}
	if len(entries) != 2 {
		t.Fatalf("entries = %d, want 2: %#v", len(entries), entries)
	}
	result, ok := entries[0].SendResult()
	if !ok || result.MessageID != "platform-1" || result.ClientID != "client-1" {
		t.Fatalf("send result = %#v ok=%v", result, ok)
	}
	references, ok := entries[1].InboundReferences()
	if !ok || len(references) != 1 || references[0].MessageID != "reply-1" {
		t.Fatalf("references = %#v ok=%v", references, ok)
	}
	if got := references[0].ReferencedMessageIDs; len(got) != 1 || got[0] != "platform-1" {
		t.Fatalf("referenced IDs = %#v", got)
	}
}

func TestReadClawbotDebugLogMissingFile(t *testing.T) {
	_, err := ReadClawbotDebugLog(filepath.Join(t.TempDir(), "absent.log"))
	if !errors.Is(err, fs.ErrNotExist) {
		t.Fatalf("err = %v, want fs.ErrNotExist", err)
	}
}

func TestDebugEntryAccessorsRejectWrongOperation(t *testing.T) {
	entry := DebugEntry{
		Operation: DebugOperationGetUpdates,
		Data:      json.RawMessage(`{"client_id":"client-1"}`),
	}
	if _, ok := entry.SendRequest(); ok {
		t.Fatal("SendRequest accepted a non-request entry")
	}
	if _, ok := entry.SendResult(); ok {
		t.Fatal("SendResult accepted a non-result entry")
	}
	if _, ok := entry.InboundReferences(); ok {
		t.Fatal("InboundReferences accepted a non-getupdates entry")
	}
}

func TestDebugEntryInboundReferencesNormalizesIDs(t *testing.T) {
	entry := DebugEntry{
		Operation: DebugOperationGetUpdatesData,
		Data: json.RawMessage(`[
			{"msg_id":" reply-1 ","referenced_msg_ids":[" platform-1 ","platform-1",""]}
		]`),
	}
	references, ok := entry.InboundReferences()
	if !ok || len(references) != 1 {
		t.Fatalf("references = %#v ok=%v", references, ok)
	}
	if references[0].MessageID != "reply-1" {
		t.Fatalf("message ID = %q", references[0].MessageID)
	}
	if got := references[0].ReferencedMessageIDs; len(got) != 1 || got[0] != "platform-1" {
		t.Fatalf("referenced IDs = %#v", got)
	}
}

func TestDebugLogRoundTripThroughWriter(t *testing.T) {
	dir := t.TempDir()
	t.Setenv("AGENT_NOTIFY_TEMP_DIR", dir)
	t.Setenv("AGENT_NOTIFY_CLAWBOT_DEBUG", "1")

	writeClawbotDebugEvent(DebugOperationSendRequest, DebugSendRequest{ClientID: "client-1"})
	writeClawbotDebugEvent(DebugOperationSendResult, DebugSendResult{MessageID: "platform-1", ClientID: "client-1"})

	entries, err := ReadClawbotDebugLog(filepath.Join(dir, "clawbot-debug.log"))
	if err != nil {
		t.Fatalf("ReadClawbotDebugLog: %v", err)
	}
	if len(entries) != 2 {
		t.Fatalf("entries = %d, want 2", len(entries))
	}
	request, ok := entries[0].SendRequest()
	if !ok || request.ClientID != "client-1" {
		t.Fatalf("send request = %#v ok=%v", request, ok)
	}
	if entries[0].Timestamp.IsZero() {
		t.Fatal("timestamp was not parsed")
	}
}

func TestDebugEntryInboundReferencesPreservesMissingIDs(t *testing.T) {
	entry := DebugEntry{
		Operation: DebugOperationGetUpdatesData,
		Data:      json.RawMessage(`[{"msg_id":"reply-1","has_reference":true,"referenced_msg_ids":[]}]`),
	}
	references, ok := entry.InboundReferences()
	if !ok || len(references) != 1 {
		t.Fatalf("references = %#v ok=%v", references, ok)
	}
	if !references[0].HasReference || len(references[0].ReferencedMessageIDs) != 0 {
		t.Fatalf("reference = %#v", references[0])
	}
}
