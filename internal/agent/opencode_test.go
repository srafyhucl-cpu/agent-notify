package agent

import (
	"path/filepath"
	"testing"

	"github.com/srafyhucl-cpu/agent-notify/internal/config"
	"github.com/srafyhucl-cpu/agent-notify/internal/marker"
	"github.com/srafyhucl-cpu/agent-notify/internal/notify"
)

func TestHandleOpenCodeMarkerAndDryRun(t *testing.T) {
	t.Setenv("AGENT_NOTIFY_CONFIG_DIR", t.TempDir())
	t.Setenv("AGENT_NOTIFY_TEMP_DIR", filepath.Join(t.TempDir(), "temp"))
	t.Setenv("AGENT_NOTIFY_QUIET", "")

	paths := config.GetPaths()
	if _, err := marker.SetMarker(paths.OpenCodeMarker, "Off"); err != nil {
		t.Fatal(err)
	}
	result := HandleOpenCode("测试", "hello", "session", 500, true, true)
	if result.Status != notify.StatusSkipped {
		t.Fatalf("marker-off status = %q, want skipped", result.Status)
	}

	if _, err := marker.SetMarker(paths.OpenCodeMarker, "On"); err != nil {
		t.Fatal(err)
	}
	result = HandleOpenCode("测试", "hello", "session", 500, true, true)
	if result.Status != notify.StatusDryRun {
		t.Fatalf("dry-run status = %q, want %q", result.Status, notify.StatusDryRun)
	}
}
