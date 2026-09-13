package reply

import (
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"sync"
	"testing"
	"time"
)

func TestRouteStoreMatchesExactAccountAndMessage(t *testing.T) {
	now := time.Date(2026, 9, 12, 10, 0, 0, 0, time.Local)
	store := NewRouteStore(filepath.Join(t.TempDir(), "routes.jsonl"))
	store.Now = func() time.Time { return now }

	route := Route{
		MessageID: "message-1",
		ClientID:  "client-1",
		BotID:     "bot-1",
		UserID:    "user-1",
		Agent:     "codex",
		SessionID: "thread-1",
	}
	if err := store.Record(route); err != nil {
		t.Fatalf("Record: %v", err)
	}

	found, err := store.Find("bot-1", "user-1", "message-1", "")
	if err != nil {
		t.Fatalf("Find message: %v", err)
	}
	if found.SessionID != "thread-1" || found.Agent != "codex" {
		t.Fatalf("unexpected route: %#v", found)
	}
	found, err = store.Find("bot-1", "user-1", "", "client-1")
	if err != nil || found.SessionID != "thread-1" {
		t.Fatalf("Find client: route=%#v err=%v", found, err)
	}
	if _, err := store.Find("bot-2", "user-1", "message-1", ""); !errors.Is(err, ErrRouteNotFound) {
		t.Fatalf("cross-account error = %v, want ErrRouteNotFound", err)
	}
	if _, err := store.Find("bot-1", "user-1", "message-2", ""); !errors.Is(err, ErrRouteNotFound) {
		t.Fatalf("unknown message error = %v, want ErrRouteNotFound", err)
	}
}

func TestRouteStoreIgnoresExpiredRoutes(t *testing.T) {
	now := time.Date(2026, 9, 12, 10, 0, 0, 0, time.Local)
	store := NewRouteStore(filepath.Join(t.TempDir(), "routes.jsonl"))
	store.Now = func() time.Time { return now }
	route := Route{
		MessageID: "message-1",
		BotID:     "bot-1",
		UserID:    "user-1",
		Agent:     "codex",
		SessionID: "thread-1",
		CreatedAt: now.Add(-2 * time.Hour),
		ExpiresAt: now.Add(-time.Minute),
	}
	if err := store.Record(route); !errors.Is(err, ErrRouteExpired) {
		t.Fatalf("Record error = %v, want ErrRouteExpired", err)
	}
}

func TestRouteStoreReportsExpiredExistingRoute(t *testing.T) {
	now := time.Date(2026, 9, 12, 10, 0, 0, 0, time.Local)
	store := NewRouteStore(filepath.Join(t.TempDir(), "routes.jsonl"))
	store.Now = func() time.Time { return now }
	if err := appendJSONLine(store.Path, Route{
		MessageID: "message-expired",
		BotID:     "bot-1",
		UserID:    "user-1",
		Agent:     "codex",
		SessionID: "thread-1",
		CreatedAt: now.Add(-DefaultRouteTTL - time.Minute),
		ExpiresAt: now.Add(-time.Minute),
	}); err != nil {
		t.Fatalf("append expired route: %v", err)
	}

	if _, err := store.Find("bot-1", "user-1", "message-expired", ""); !errors.Is(err, ErrRouteExpired) {
		t.Fatalf("Find error = %v, want ErrRouteExpired", err)
	}
}

func TestRouteStoreNormalizesPersistedIdentifiers(t *testing.T) {
	store := NewRouteStore(filepath.Join(t.TempDir(), "routes.jsonl"))
	if err := store.Record(Route{
		MessageID: " message-1 ",
		ClientID:  " client-1 ",
		BotID:     " bot-1 ",
		UserID:    " user-1 ",
		Agent:     " codex ",
		SessionID: " thread-1 ",
	}); err != nil {
		t.Fatalf("Record: %v", err)
	}

	route, err := store.Find("bot-1", "user-1", "message-1", "")
	if err != nil {
		t.Fatalf("Find: %v", err)
	}
	if route.MessageID != "message-1" || route.SessionID != "thread-1" {
		t.Fatalf("route was not normalized: %#v", route)
	}
}

func TestRouteStoreConcurrentRecordsAreNotLost(t *testing.T) {
	const writers = 32
	store := NewRouteStore(filepath.Join(t.TempDir(), "routes.jsonl"))
	start := make(chan struct{})
	var wait sync.WaitGroup
	errs := make(chan error, writers)
	for index := 0; index < writers; index++ {
		wait.Add(1)
		go func() {
			defer wait.Done()
			<-start
			errs <- store.Record(Route{
				MessageID: fmt.Sprintf("message-%02d", index),
				BotID:     "bot-1",
				UserID:    "user-1",
				Agent:     "codex",
				SessionID: fmt.Sprintf("thread-%02d", index),
			})
		}()
	}
	close(start)
	wait.Wait()
	close(errs)
	for err := range errs {
		if err != nil {
			t.Fatalf("concurrent Record: %v", err)
		}
	}

	routes, err := store.load()
	if err != nil {
		t.Fatalf("load routes: %v", err)
	}
	if len(routes) != writers {
		t.Fatalf("routes = %d, want %d", len(routes), writers)
	}
}

func TestRouteStoreRejectsAmbiguousMatches(t *testing.T) {
	now := time.Date(2026, 9, 12, 10, 0, 0, 0, time.Local)
	store := NewRouteStore(filepath.Join(t.TempDir(), "routes.jsonl"))
	store.Now = func() time.Time { return now }
	for _, sessionID := range []string{"thread-1", "thread-2"} {
		if err := store.Record(Route{
			MessageID: "duplicate",
			BotID:     "bot-1",
			UserID:    "user-1",
			Agent:     "codex",
			SessionID: sessionID,
		}); err != nil {
			t.Fatalf("Record %s: %v", sessionID, err)
		}
	}
	if _, err := store.Find("bot-1", "user-1", "duplicate", ""); !errors.Is(err, ErrRouteAmbiguous) {
		t.Fatalf("Find error = %v, want ErrRouteAmbiguous", err)
	}
}

func TestRouteStoreDeduplicatesIdenticalTargets(t *testing.T) {
	now := time.Date(2026, 9, 12, 10, 0, 0, 0, time.Local)
	store := NewRouteStore(filepath.Join(t.TempDir(), "routes.jsonl"))
	store.Now = func() time.Time { return now }
	for range 2 {
		if err := store.Record(Route{
			MessageID: "message-1",
			ClientID:  "client-1",
			BotID:     "bot-1",
			UserID:    "user-1",
			Agent:     "codex",
			SessionID: "thread-1",
		}); err != nil {
			t.Fatalf("Record: %v", err)
		}
	}
	route, err := store.Find("bot-1", "user-1", "message-1", "")
	if err != nil {
		t.Fatalf("Find: %v", err)
	}
	if route.SessionID != "thread-1" {
		t.Fatalf("route = %#v", route)
	}
}

func TestRouteStoreIgnoresIncompletePersistedRoutes(t *testing.T) {
	store := NewRouteStore(filepath.Join(t.TempDir(), "routes.jsonl"))
	writeTestJSONL(t, store.Path,
		map[string]any{
			"messageID": "missing-scope",
			"agent":     "codex",
			"sessionID": "thread-1",
		},
		map[string]any{
			"botID":     "bot-1",
			"userID":    "user-1",
			"agent":     "codex",
			"sessionID": "thread-2",
		},
	)

	routes, err := store.load()
	if err != nil {
		t.Fatalf("load routes: %v", err)
	}
	if len(routes) != 0 {
		t.Fatalf("routes = %#v, want no incomplete routes", routes)
	}
	if _, err := store.Find("bot-1", "user-1", "missing-scope", ""); !errors.Is(err, ErrRouteNotFound) {
		t.Fatalf("Find error = %v, want ErrRouteNotFound", err)
	}
}

func TestRouteStoreIgnoresMalformedLines(t *testing.T) {
	store := NewRouteStore(filepath.Join(t.TempDir(), "routes.jsonl"))
	if err := os.WriteFile(store.Path, []byte("not-json\n"), privateFilePerm); err != nil {
		t.Fatalf("write malformed route: %v", err)
	}
	if err := appendJSONLine(store.Path, Route{
		MessageID: "message-1",
		BotID:     "bot-1",
		UserID:    "user-1",
		Agent:     "codex",
		SessionID: "thread-1",
	}); err != nil {
		t.Fatalf("append valid route: %v", err)
	}

	routes, err := store.load()
	if err != nil {
		t.Fatalf("load routes: %v", err)
	}
	if len(routes) != 1 || routes[0].SessionID != "thread-1" {
		t.Fatalf("routes = %#v, want only valid route", routes)
	}
}

func TestRouteStoreRejectsCrossFieldIdentifierCollision(t *testing.T) {
	now := time.Date(2026, 9, 12, 10, 0, 0, 0, time.Local)
	store := NewRouteStore(filepath.Join(t.TempDir(), "routes.jsonl"))
	store.Now = func() time.Time { return now }
	for _, route := range []Route{
		{
			MessageID: "shared-identifier",
			BotID:     "bot-1",
			UserID:    "user-1",
			Agent:     "codex",
			SessionID: "thread-1",
		},
		{
			ClientID:  "shared-identifier",
			BotID:     "bot-1",
			UserID:    "user-1",
			Agent:     "codex",
			SessionID: "thread-2",
		},
	} {
		if err := store.Record(route); err != nil {
			t.Fatalf("Record: %v", err)
		}
	}

	if _, err := store.Find("bot-1", "user-1", "shared-identifier", "shared-identifier"); !errors.Is(err, ErrRouteAmbiguous) {
		t.Fatalf("Find error = %v, want ErrRouteAmbiguous", err)
	}
}

func TestRouteStoreListActiveFiltersAccountAndExpiry(t *testing.T) {
	now := time.Date(2026, 9, 12, 10, 0, 0, 0, time.Local)
	store := NewRouteStore(filepath.Join(t.TempDir(), "routes.jsonl"))
	store.Now = func() time.Time { return now }
	if err := store.Record(Route{
		MessageID: "message-current",
		BotID:     "bot-1",
		UserID:    "user-1",
		Agent:     "codex",
		SessionID: "thread-current",
	}); err != nil {
		t.Fatalf("Record current: %v", err)
	}
	if err := store.Record(Route{
		MessageID: "message-other",
		BotID:     "bot-2",
		UserID:    "user-1",
		Agent:     "codex",
		SessionID: "thread-other",
	}); err != nil {
		t.Fatalf("Record other: %v", err)
	}
	if err := appendJSONLine(store.Path, Route{
		MessageID: "message-expired",
		BotID:     "bot-1",
		UserID:    "user-1",
		Agent:     "codex",
		SessionID: "thread-expired",
		CreatedAt: now.Add(-2 * time.Hour),
		ExpiresAt: now.Add(-time.Minute),
	}); err != nil {
		t.Fatalf("append expired: %v", err)
	}

	routes, err := store.ListActive("bot-1", "user-1")
	if err != nil {
		t.Fatalf("ListActive: %v", err)
	}
	if len(routes) != 1 || routes[0].SessionID != "thread-current" {
		t.Fatalf("routes = %#v", routes)
	}
	if !routes[0].ExpiresAt.After(now) {
		t.Fatalf("expiresAt = %v, want future", routes[0].ExpiresAt)
	}
}
