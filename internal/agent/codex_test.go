package agent

import (
	"testing"
)

func TestConvertCodexArgs(t *testing.T) {
	// Case 1: Standard event
	jsonArg := `{"input-messages":["重构 LinkWeixin 模块并测试"],"last-assistant-message":"已经完成重构，所有单元测试通过。"}`
	title, summary := ConvertCodexArgs([]string{"turn-ended", jsonArg})

	expectedTitle := "【codex】重构 LinkWeixin 模块并测试"
	expectedSummary := "已经完成重构，所有单元测试通过。"

	if title != expectedTitle {
		t.Errorf("title = %q, want %q", title, expectedTitle)
	}
	if summary != expectedSummary {
		t.Errorf("summary = %q, want %q", summary, expectedSummary)
	}

	// Case 2: Long input message (> 30 runes)
	longInput := "这是一段非常非常非常非常非常非常非常非常非常非常非常非常长的主题任务描述"
	jsonArgLong := `{"input-messages":["` + longInput + `"],"last-assistant-message":"完成"}`
	titleLong, _ := ConvertCodexArgs([]string{jsonArgLong})
	runes := []rune(longInput)
	expectedLongTitle := "【codex】" + string(runes[:30]) + "…"
	if titleLong != expectedLongTitle {
		t.Errorf("titleLong = %q, want %q", titleLong, expectedLongTitle)
	}

	// Case 3: Empty / non-json args
	titleEmpty, summaryEmpty := ConvertCodexArgs([]string{"turn-ended", "something-else"})
	if titleEmpty != "【codex】跑完了" {
		t.Errorf("titleEmpty = %q, want %q", titleEmpty, "【codex】跑完了")
	}
	if summaryEmpty != "" {
		t.Errorf("summaryEmpty = %q, want %q", summaryEmpty, "")
	}
}

func TestHandleCodexDryRun(t *testing.T) {
	t.Setenv("CODEX_NOTIFY_DRYRUN", "1")
	jsonArg := `{"input-messages":["DryRun测试"],"last-assistant-message":"完成"}`
	// Should not panic or error
	HandleCodex([]string{"turn-ended", jsonArg, "-dry-run"})
}
