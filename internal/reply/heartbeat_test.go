package reply

import (
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"
)

func TestClassifyHeartbeat(t *testing.T) {
	now := time.Now()
	tests := []struct {
		name    string
		state   heartbeatState
		capable bool
		fresh   bool
		valid   bool
	}{
		{name: "zero timestamp", state: heartbeatState{Ready: true}, capable: true},
		{name: "future timestamp", state: heartbeatState{Ready: true, Timestamp: now.Add(time.Minute)}, capable: true},
		{name: "fresh capable", state: heartbeatState{Ready: true, Timestamp: now}, capable: true, fresh: true, valid: true},
		{name: "fresh unsupported", state: heartbeatState{Ready: false, Timestamp: now}, fresh: true, valid: true},
		{name: "stale capable", state: heartbeatState{Ready: true, Timestamp: now.Add(-time.Minute)}, capable: true, valid: true},
	}
	for _, test := range tests {
		t.Run(test.name, func(t *testing.T) {
			capable, fresh, valid := classifyHeartbeat(test.state, now, devinHeartbeatMaxAge, devinHeartbeatFutureSkew)
			if capable != test.capable || fresh != test.fresh || valid != test.valid {
				t.Fatalf("classifyHeartbeat = (%v, %v, %v), want (%v, %v, %v)",
					capable, fresh, valid, test.capable, test.fresh, test.valid)
			}
		})
	}
}

func TestCheckHeartbeatEntriesPriority(t *testing.T) {
	now := time.Now()
	heartbeatDir := func(dir string) string { return filepath.Join(dir, "heartbeats") }
	writeRaw := func(t *testing.T, dir, name string, data []byte) {
		t.Helper()
		path := filepath.Join(heartbeatDir(dir), name)
		if err := os.MkdirAll(filepath.Dir(path), privateDirPerm); err != nil {
			t.Fatal(err)
		}
		if err := os.WriteFile(path, data, privateFilePerm); err != nil {
			t.Fatal(err)
		}
	}
	check := func(t *testing.T, dir string) error {
		t.Helper()
		entries, err := os.ReadDir(heartbeatDir(dir))
		if err != nil {
			t.Fatal(err)
		}
		return checkHeartbeatEntries(heartbeatDir(dir), entries, parseDevinHeartbeat, now, devinHeartbeatMaxAge, devinHeartbeatFutureSkew, devinHeartbeatMessages)
	}

	t.Run("fresh capable wins", func(t *testing.T) {
		dir := t.TempDir()
		writeTestJSON(t, filepath.Join(heartbeatDir(dir), "ok.json"), devinHeartbeat{Ready: true, Timestamp: now})
		if err := check(t, dir); err != nil {
			t.Fatalf("checkHeartbeatEntries = %v, want nil", err)
		}
	})

	t.Run("unsupported beats offline", func(t *testing.T) {
		dir := t.TempDir()
		writeTestJSON(t, filepath.Join(heartbeatDir(dir), "stale.json"), devinHeartbeat{Ready: true, Timestamp: now.Add(-time.Minute)})
		writeTestJSON(t, filepath.Join(heartbeatDir(dir), "unsupported.json"), devinHeartbeat{Ready: false, Timestamp: now})
		if err := check(t, dir); err == nil || !strings.Contains(err.Error(), "精确回复能力") {
			t.Fatalf("checkHeartbeatEntries = %v, want unsupported message", err)
		}
	})

	t.Run("offline", func(t *testing.T) {
		dir := t.TempDir()
		writeTestJSON(t, filepath.Join(heartbeatDir(dir), "stale.json"), devinHeartbeat{Ready: true, Timestamp: now.Add(-time.Minute)})
		if err := check(t, dir); err == nil || !strings.Contains(err.Error(), "已离线") {
			t.Fatalf("checkHeartbeatEntries = %v, want offline message", err)
		}
	})

	t.Run("invalid time", func(t *testing.T) {
		dir := t.TempDir()
		writeTestJSON(t, filepath.Join(heartbeatDir(dir), "zero.json"), devinHeartbeat{Ready: true})
		if err := check(t, dir); err == nil || !strings.Contains(err.Error(), "心跳时间无效") {
			t.Fatalf("checkHeartbeatEntries = %v, want invalid-time message", err)
		}
	})

	t.Run("malformed", func(t *testing.T) {
		dir := t.TempDir()
		writeRaw(t, dir, "broken.json", []byte("{"))
		if err := check(t, dir); err == nil || !strings.Contains(err.Error(), "状态无效") {
			t.Fatalf("checkHeartbeatEntries = %v, want malformed message", err)
		}
	})

	t.Run("not running ignores non-json and directories", func(t *testing.T) {
		dir := t.TempDir()
		writeRaw(t, dir, "note.txt", []byte("x"))
		if err := os.MkdirAll(filepath.Join(heartbeatDir(dir), "nested"), privateDirPerm); err != nil {
			t.Fatal(err)
		}
		if err := check(t, dir); err == nil || !strings.Contains(err.Error(), "未运行") {
			t.Fatalf("checkHeartbeatEntries = %v, want not-running message", err)
		}
	})
}
