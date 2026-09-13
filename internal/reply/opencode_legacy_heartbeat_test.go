package reply

import (
	"path/filepath"
	"testing"
	"time"
)

func TestOpenCodeLegacyHeartbeatRemainsCompatible(t *testing.T) {
	dir := t.TempDir()
	writeTestJSON(t, filepath.Join(dir, openCodeLegacyHeartbeatFile), openCodeHeartbeat{
		Ready:     true,
		Timestamp: time.Now(),
	})
	if err := requireOpenCodeHeartbeat(dir, time.Now()); err != nil {
		t.Fatalf("requireOpenCodeHeartbeat: %v", err)
	}
}

func TestOpenCodeLegacyHeartbeatSurvivesLeaseDirectory(t *testing.T) {
	dir := t.TempDir()
	now := time.Now()
	writeOpenCodeHeartbeat(t, dir, "stale-instance", openCodeHeartbeat{
		Ready:     true,
		Timestamp: now.Add(-time.Minute),
	})
	writeTestJSON(t, filepath.Join(dir, openCodeLegacyHeartbeatFile), openCodeHeartbeat{
		Ready:     true,
		Timestamp: now,
	})

	if err := requireOpenCodeHeartbeat(dir, now); err != nil {
		t.Fatalf("requireOpenCodeHeartbeat with legacy lease: %v", err)
	}
}
