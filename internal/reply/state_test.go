package reply

import (
	"os"
	"path/filepath"
	"sync"
	"testing"
	"time"
)

func TestStateStoreClaimsKeyOnce(t *testing.T) {
	path := filepath.Join(t.TempDir(), "state.jsonl")
	store := NewStateStore(path)
	claimed, err := store.Claim("message:1")
	if err != nil {
		t.Fatalf("Claim first: %v", err)
	}
	if !claimed {
		t.Fatal("first claim was rejected")
	}
	claimed, err = store.Claim("message:1")
	if err != nil {
		t.Fatalf("Claim second: %v", err)
	}
	if claimed {
		t.Fatal("duplicate claim was accepted")
	}
	claimed, err = NewStateStore(path).Claim("message:1")
	if err != nil {
		t.Fatalf("Claim from new store: %v", err)
	}
	if claimed {
		t.Fatal("duplicate claim was accepted after reopening store")
	}
	if err := store.Mark("message:1", replyStateSent); err != nil {
		t.Fatalf("Mark: %v", err)
	}
}

func TestStateStoreClaimExpiresAfterTTL(t *testing.T) {
	path := filepath.Join(t.TempDir(), "state.jsonl")
	now := time.Date(2026, 9, 12, 10, 0, 0, 0, time.Local)
	store := NewStateStore(path)
	store.Now = func() time.Time { return now }

	claimed, err := store.Claim("message:old")
	if err != nil || !claimed {
		t.Fatalf("first Claim = %v, %v", claimed, err)
	}

	store.Now = func() time.Time { return now.Add(DefaultStateTTL + time.Second) }
	claimed, err = store.Claim("message:old")
	if err != nil || !claimed {
		t.Fatalf("expired Claim = %v, %v", claimed, err)
	}
}

func TestStateStoreConcurrentClaimsOnlyOneWinner(t *testing.T) {
	path := filepath.Join(t.TempDir(), "state.jsonl")
	const workers = 16
	start := make(chan struct{})
	results := make(chan bool, workers)
	errs := make(chan error, workers)
	var wg sync.WaitGroup
	for i := 0; i < workers; i++ {
		wg.Add(1)
		go func() {
			defer wg.Done()
			<-start
			claimed, err := NewStateStore(path).Claim("message:concurrent")
			results <- claimed
			errs <- err
		}()
	}
	close(start)
	wg.Wait()
	close(results)
	close(errs)

	winners := 0
	for claimed := range results {
		if claimed {
			winners++
		}
	}
	for err := range errs {
		if err != nil {
			t.Fatalf("concurrent Claim: %v", err)
		}
	}
	if winners != 1 {
		t.Fatalf("winners = %d, want 1", winners)
	}
}

func TestStateStoreIgnoresMalformedLines(t *testing.T) {
	path := filepath.Join(t.TempDir(), "state.jsonl")
	if err := os.WriteFile(path, []byte("not-json\n"), privateFilePerm); err != nil {
		t.Fatalf("write malformed state: %v", err)
	}
	now := time.Date(2026, 9, 12, 10, 0, 0, 0, time.Local)
	if err := appendJSONLineUnlocked(path, stateEvent{
		Key:       "message:1",
		Status:    replyStateClaimed,
		Timestamp: now,
	}); err != nil {
		t.Fatalf("append valid state: %v", err)
	}

	store := NewStateStore(path)
	store.Now = func() time.Time { return now.Add(time.Minute) }
	claimed, err := store.Claim("message:1")
	if err != nil {
		t.Fatalf("Claim existing key: %v", err)
	}
	if claimed {
		t.Fatal("malformed line caused a valid active claim to be ignored")
	}
}
