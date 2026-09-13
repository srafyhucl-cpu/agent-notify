package reply

import (
	"encoding/json"
	"fmt"
	"os"
	"strings"
	"time"

	"github.com/srafyhucl-cpu/agent-notify/internal/config"
)

const (
	replyStateClaimed = "claimed"
	replyStateSent    = "sent"
	replyStateFailed  = "failed"
)

// DefaultStateTTL bounds replay suppression for inbound replies.
const DefaultStateTTL = DefaultRouteTTL

type stateEvent struct {
	Key       string    `json:"key"`
	Status    string    `json:"status"`
	Timestamp time.Time `json:"timestamp"`
}

// StateStore implements at-most-once claims for inbound reply messages.
type StateStore struct {
	Path string
	TTL  time.Duration
	Now  func() time.Time
}

func (s *StateStore) ttl() time.Duration {
	if s.TTL > 0 {
		return s.TTL
	}
	return DefaultStateTTL
}

func NewStateStore(path string) *StateStore {
	if strings.TrimSpace(path) == "" {
		path = config.GetPaths().ReplyStateFile
	}
	return &StateStore{Path: path, TTL: DefaultStateTTL, Now: time.Now}
}

// Claim atomically records a new inbound reply key. It returns false when the
// key has an active claim, including a previous failed attempt within the TTL.
func (s *StateStore) Claim(key string) (bool, error) {
	key = strings.TrimSpace(key)
	if key == "" {
		return false, fmt.Errorf("reply: empty deduplication key")
	}
	claimed := false
	err := withFileLock(s.Path+".lock", func() error {
		exists, err := s.hasLocked(key)
		if err != nil || exists {
			return err
		}
		if err := s.appendLocked(key, replyStateClaimed); err != nil {
			return err
		}
		claimed = true
		return nil
	})
	return claimed, err
}

func (s *StateStore) Mark(key, status string) error {
	key = strings.TrimSpace(key)
	if key == "" {
		return fmt.Errorf("reply: empty deduplication key")
	}
	switch status {
	case replyStateSent, replyStateFailed:
	default:
		return fmt.Errorf("reply: invalid state %q", status)
	}
	return withFileLock(s.Path+".lock", func() error {
		return s.appendLocked(key, status)
	})
}

func (s *StateStore) hasLocked(key string) (bool, error) {
	file, err := os.Open(s.Path)
	if err != nil {
		if os.IsNotExist(err) {
			return false, nil
		}
		return false, err
	}
	defer file.Close()

	now := s.now()
	scanner := newJSONLScanner(file)
	for scanner.Scan() {
		line := strings.TrimSpace(scanner.Text())
		if line == "" {
			continue
		}
		var event stateEvent
		if err := json.Unmarshal([]byte(line), &event); err != nil {
			continue
		}
		if event.Key == key {
			if s.eventActive(event, now) {
				return true, nil
			}
		}
	}
	if err := scanner.Err(); err != nil {
		return false, fmt.Errorf("reply: read state: %w", err)
	}
	return false, nil
}

func (s *StateStore) eventActive(event stateEvent, now time.Time) bool {
	if event.Timestamp.IsZero() {
		return false
	}
	return event.Timestamp.Add(s.ttl()).After(now)
}

func (s *StateStore) appendLocked(key, status string) error {
	now := s.now()
	event := stateEvent{
		Key:       key,
		Status:    status,
		Timestamp: now,
	}
	if err := appendJSONLineUnlocked(s.Path, event); err != nil {
		return err
	}
	_ = compactJSONL(s.Path, s.compactionKeep(now), defaultJSONLCompactionOptions)
	return nil
}

func (s *StateStore) compactionKeep(now time.Time) func([]byte) bool {
	return func(raw []byte) bool {
		var event stateEvent
		if err := json.Unmarshal(raw, &event); err != nil {
			return false
		}
		return s.eventActive(event, now)
	}
}

func (s *StateStore) now() time.Time {
	if s.Now != nil {
		return s.Now()
	}
	return time.Now()
}
