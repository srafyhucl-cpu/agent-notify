//go:build windows

package ui

import "testing"

// 修复结果必须由 UI 线程应用：成功时清掉首次接入错误，失败时只更新修复提示。
func TestApplyRepairDone(t *testing.T) {
	success := WidgetApp{repairing: true, setupError: "旧错误", repairError: "旧失败"}
	successCh := make(chan repairResult, 1)
	successCh <- repairResult{}
	success.repairDone = successCh
	success.applyRepairDone()
	if success.repairing {
		t.Fatal("修复完成后 repairing 应为 false")
	}
	if success.setupError != "" {
		t.Fatalf("修复成功后 setupError = %q, want empty", success.setupError)
	}
	if success.repairError != "" {
		t.Fatalf("修复成功后 repairError = %q, want empty", success.repairError)
	}
	if success.repairDone != nil {
		t.Fatal("结果通道未清理")
	}

	failure := WidgetApp{repairing: true, setupError: "旧错误"}
	failureCh := make(chan repairResult, 1)
	failureCh <- repairResult{setupFailed: true, errors: []string{"首次接入：hook 被占用", "Codex：写入失败"}}
	failure.repairDone = failureCh
	failure.applyRepairDone()
	if failure.repairing {
		t.Fatal("修复完成后 repairing 应为 false")
	}
	if failure.setupError != "旧错误" {
		t.Fatalf("首次接入失败时 setupError 不应被清空: %q", failure.setupError)
	}
	if want := "首次接入：hook 被占用；Codex：写入失败"; failure.repairError != want {
		t.Fatalf("repairError = %q, want %q", failure.repairError, want)
	}

	// 没有结果时不应改动状态（例如消息早于通道就绪）
	stale := WidgetApp{repairing: true, repairError: "保留"}
	stale.applyRepairDone()
	if !stale.repairing || stale.repairError != "保留" {
		t.Fatal("无结果时不应改动修复状态")
	}
}

// 配对码输入区必须落在登录卡片内且不与提交按钮重叠。
func TestLoginVerifyRectsStayInsideCard(t *testing.T) {
	card := subviewCardRect()
	field, submit := loginVerifyRects()
	for name, rect := range map[string]RECT{"输入框": field, "提交按钮": submit} {
		if rect.Left < card.Left || rect.Right > card.Right || rect.Top < card.Top || rect.Bottom > card.Bottom {
			t.Fatalf("%s %+v 超出登录卡片 %+v", name, rect, card)
		}
	}
	if field.Bottom >= submit.Top {
		t.Fatalf("输入框 %+v 与提交按钮 %+v 重叠", field, submit)
	}
	edit := verifyEditRect()
	if edit.Left < field.Left || edit.Right > field.Right || edit.Top < field.Top || edit.Bottom > field.Bottom {
		t.Fatalf("EDIT 控件 %+v 超出输入框底框 %+v", edit, field)
	}
}

// 历史列表的行命中必须与绘制行一致：卡片底部空白、行间空隙与卡片外都不应选中条目。
func TestHistoryRowIndexAtBounds(t *testing.T) {
	listCard := RECT{14, 48, 386, 294}
	cases := []struct {
		name string
		y    int32
		want int
	}{
		{"第一行中心", 72, 0},
		{"第五行中心", 232, 4},
		{"第六行中心", 272, 5},
		{"行间空隙", 91, -1},
		{"末行下方的卡片空白", 292, -1},
		{"卡片上方", 40, -1},
		{"卡片下方", 300, -1},
	}
	for _, tc := range cases {
		if got := historyRowIndexAt(100, tc.y, listCard, historyListPageSize); got != tc.want {
			t.Fatalf("%s：historyRowIndexAt(y=%d) = %d, want %d", tc.name, tc.y, got, tc.want)
		}
	}
	if got := historyRowIndexAt(400, 72, listCard, historyListPageSize); got != -1 {
		t.Fatalf("卡片右侧不应命中行：got %d", got)
	}
}
