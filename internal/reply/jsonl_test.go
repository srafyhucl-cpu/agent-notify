package reply

import (
	"encoding/json"
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"
)

func testJSONLCompactionOptions() jsonlCompactionOptions {
	return jsonlCompactionOptions{
		MinBytes:          1,
		MinReclaimedBytes: 1,
		MinReclaimedRatio: 0,
	}
}

func writeTestJSONL(t *testing.T, path string, values ...any) {
	t.Helper()
	lines := make([]string, 0, len(values))
	for _, value := range values {
		data, err := json.Marshal(value)
		if err != nil {
			t.Fatalf("marshal JSONL fixture: %v", err)
		}
		lines = append(lines, string(data))
	}
	if err := os.WriteFile(path, []byte(strings.Join(lines, "\n")+"\n"), privateFilePerm); err != nil {
		t.Fatalf("write JSONL fixture: %v", err)
	}
}

func TestCompactJSONLRemovesRejectedLines(t *testing.T) {
	path := filepath.Join(t.TempDir(), "state.jsonl")
	if err := os.WriteFile(path, []byte("{\"keep\":true}\n{\"keep\":false}\nnot-json\n"), privateFilePerm); err != nil {
		t.Fatal(err)
	}

	err := compactJSONL(path, func(raw []byte) bool {
		var value struct {
			Keep bool `json:"keep"`
		}
		return json.Unmarshal(raw, &value) == nil && value.Keep
	}, testJSONLCompactionOptions())
	if err != nil {
		t.Fatalf("compactJSONL: %v", err)
	}
	data, err := os.ReadFile(path)
	if err != nil {
		t.Fatal(err)
	}
	if string(data) != "{\"keep\":true}\n" {
		t.Fatalf("compacted data = %q", data)
	}
	leftovers, err := filepath.Glob(path + ".compact-*")
	if err != nil {
		t.Fatal(err)
	}
	if len(leftovers) != 0 {
		t.Fatalf("compaction temporary files remain: %#v", leftovers)
	}
}

func TestRouteStoreCompactionDropsExpiredRoutes(t *testing.T) {
	now := time.Date(2026, 9, 12, 10, 0, 0, 0, time.Local)
	store := NewRouteStore(filepath.Join(t.TempDir(), "routes.jsonl"))
	store.Now = func() time.Time { return now }
	writeTestJSONL(t, store.Path,
		Route{
			MessageID: "expired",
			BotID:     "bot-1",
			UserID:    "user-1",
			Agent:     "codex",
			SessionID: "thread-old",
			CreatedAt: now.Add(-2 * DefaultRouteTTL),
			ExpiresAt: now.Add(-time.Minute),
		},
		Route{
			MessageID: "active",
			BotID:     "bot-1",
			UserID:    "user-1",
			Agent:     "codex",
			SessionID: "thread-current",
			CreatedAt: now,
			ExpiresAt: now.Add(time.Hour),
		},
	)

	if err := compactJSONL(store.Path, store.compactionKeep(now), testJSONLCompactionOptions()); err != nil {
		t.Fatalf("compact routes: %v", err)
	}
	routes, err := store.load()
	if err != nil {
		t.Fatalf("load routes: %v", err)
	}
	if len(routes) != 1 || routes[0].MessageID != "active" {
		t.Fatalf("routes after compaction = %#v", routes)
	}
}

func TestStateStoreCompactionDropsExpiredEvents(t *testing.T) {
	now := time.Date(2026, 9, 12, 10, 0, 0, 0, time.Local)
	store := NewStateStore(filepath.Join(t.TempDir(), "state.jsonl"))
	store.Now = func() time.Time { return now }
	writeTestJSONL(t, store.Path,
		stateEvent{Key: "old", Status: replyStateSent, Timestamp: now.Add(-DefaultStateTTL - time.Second)},
		stateEvent{Key: "active", Status: replyStateClaimed, Timestamp: now},
	)

	if err := compactJSONL(store.Path, store.compactionKeep(now), testJSONLCompactionOptions()); err != nil {
		t.Fatalf("compact state: %v", err)
	}
	data, err := os.ReadFile(store.Path)
	if err != nil {
		t.Fatal(err)
	}
	if strings.Contains(string(data), `"key":"old"`) || !strings.Contains(string(data), `"key":"active"`) {
		t.Fatalf("compacted state = %s", data)
	}
}

func TestRouteStoreCompactionDropsIncompleteRoutes(t *testing.T) {
	now := time.Date(2026, 9, 12, 10, 0, 0, 0, time.Local)
	store := NewRouteStore(filepath.Join(t.TempDir(), "routes.jsonl"))
	store.Now = func() time.Time { return now }
	writeTestJSONL(t, store.Path,
		map[string]any{
			"messageID": "missing-scope",
			"agent":     "codex",
			"sessionID": "thread-1",
		},
		Route{
			MessageID: "active",
			BotID:     "bot-1",
			UserID:    "user-1",
			Agent:     "codex",
			SessionID: "thread-current",
			CreatedAt: now,
			ExpiresAt: now.Add(time.Hour),
		},
	)

	if err := compactJSONL(store.Path, store.compactionKeep(now), testJSONLCompactionOptions()); err != nil {
		t.Fatalf("compact routes: %v", err)
	}
	data, err := os.ReadFile(store.Path)
	if err != nil {
		t.Fatal(err)
	}
	if strings.Contains(string(data), "missing-scope") || !strings.Contains(string(data), "thread-current") {
		t.Fatalf("compacted routes = %s", data)
	}
}
