package agent

import (
	"path/filepath"
	"testing"

	"github.com/srafyhucl-cpu/agent-notify/internal/config"
	"github.com/srafyhucl-cpu/agent-notify/internal/marker"
	"github.com/srafyhucl-cpu/agent-notify/internal/notify"
)

func TestConvertCodexArgs(t *testing.T) {
	jsonArg := `{"input-messages":["重构 Agent-notify 模块并测试"],"last-assistant-message":"已经完成重构，所有单元测试通过。"}`
	title, summary := ConvertCodexArgs([]string{"turn-ended", jsonArg})

	if title != "【codex】重构 Agent-notify 模块并测试" {
		t.Fatalf("title = %q", title)
	}
	if summary != "已经完成重构，所有单元测试通过。" {
		t.Fatalf("summary = %q", summary)
	}

	longInput := "这是一段非常非常非常非常非常非常非常非常非常非常非常非常长的主题任务描述"
	titleLong, _ := ConvertCodexArgs([]string{`{"input-messages":["` + longInput + `"],"last-assistant-message":"完成"}`})
	expected := "【codex】" + string([]rune(longInput)[:30]) + "…"
	if titleLong != expected {
		t.Fatalf("titleLong = %q, want %q", titleLong, expected)
	}

	titleEmpty, summaryEmpty := ConvertCodexArgs([]string{"turn-ended", "something-else"})
	if titleEmpty != "【codex】跑完了" || summaryEmpty != "" {
		t.Fatalf("empty event = %q / %q", titleEmpty, summaryEmpty)
	}
}

func TestHandleCodexDryRun(t *testing.T) {
	t.Setenv("AGENT_NOTIFY_CONFIG_DIR", t.TempDir())
	t.Setenv("AGENT_NOTIFY_TEMP_DIR", filepath.Join(t.TempDir(), "temp"))
	t.Setenv("LOCALAPPDATA", t.TempDir())
	t.Setenv("AGENT_NOTIFY_CODEX_DRYRUN", "1")
	t.Setenv("AGENT_NOTIFY_QUIET", "")

	result := HandleCodex([]string{"turn-ended", `{"input-messages":["DryRun测试"],"last-assistant-message":"完成"}`})
	if result.Status != notify.StatusDryRun {
		t.Fatalf("Status = %q, want %q", result.Status, notify.StatusDryRun)
	}
}

func TestHandleCodexDoNotDisturb(t *testing.T) {
	t.Setenv("AGENT_NOTIFY_CONFIG_DIR", t.TempDir())
	t.Setenv("AGENT_NOTIFY_TEMP_DIR", filepath.Join(t.TempDir(), "temp"))
	t.Setenv("LOCALAPPDATA", t.TempDir())
	t.Setenv("AGENT_NOTIFY_CODEX_DRYRUN", "1")
	t.Setenv("AGENT_NOTIFY_QUIET", "")

	result := HandleCodex([]string{"turn-ended", `{"input-messages":["[勿扰] 夜间任务"],"last-assistant-message":"完成"}`})
	if result.Status != notify.StatusSkipped || result.Error != "标题包含勿扰标记" {
		t.Fatalf("do-not-disturb result = %#v", result)
	}
}

func TestHandleCodexMarkerRecordsSkipped(t *testing.T) {
	t.Setenv("AGENT_NOTIFY_CONFIG_DIR", t.TempDir())
	t.Setenv("AGENT_NOTIFY_TEMP_DIR", filepath.Join(t.TempDir(), "temp"))
	t.Setenv("LOCALAPPDATA", t.TempDir())
	t.Setenv("AGENT_NOTIFY_CODEX_DRYRUN", "")
	t.Setenv("AGENT_NOTIFY_QUIET", "")

	paths := config.GetPaths()
	if _, err := marker.SetMarker(paths.CodexMarker, "Off"); err != nil {
		t.Fatal(err)
	}

	result := HandleCodex([]string{"turn-ended", `{"input-messages":["Marker测试"],"last-assistant-message":"完成"}`})
	if result.Status != notify.StatusSkipped {
		t.Fatalf("marker result = %#v", result)
	}
	history, err := notify.GetHistory(10, "")
	if err != nil {
		t.Fatalf("GetHistory: %v", err)
	}
	if len(history) != 1 || history[0].Status != notify.StatusSkipped || history[0].Agent != "codex" {
		t.Fatalf("marker history = %#v", history)
	}
}
