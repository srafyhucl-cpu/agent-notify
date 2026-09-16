//go:build windows

package ui

import (
	"context"
	"fmt"
	"os"
	"strings"
	"time"

	"github.com/srafyhucl-cpu/agent-notify/internal/agent"
	"github.com/srafyhucl-cpu/agent-notify/internal/agentmeta"
	"github.com/srafyhucl-cpu/agent-notify/internal/integration"
	"github.com/srafyhucl-cpu/agent-notify/internal/notify"
)

type WidgetView int

const (
	WidgetViewDashboard WidgetView = iota
	WidgetViewRepair
	WidgetViewHistory
	WidgetViewSettings
	WidgetViewLogin
)

type viewSubLayout struct {
	back   RECT
	title  RECT
	close  RECT
	card   RECT
	left   RECT
	right  RECT
	extra  RECT
	extra2 RECT
}

func subviewCommonHeader() (RECT, RECT, RECT) {
	return RECT{14, 12, 42, 38}, RECT{48, 12, 360, 38}, RECT{368, 12, 394, 38}
}

// isSubviewHeaderDrag checks if the point is in the draggable header region of a subview.
func isSubviewHeaderDrag(x, y int32, extraButtons ...RECT) bool {
	if y < 0 || y >= 48 {
		return false
	}
	backRect, _, closeRect := subviewCommonHeader()
	if pointInRect(x, y, backRect) || pointInRect(x, y, closeRect) {
		return false
	}
	for _, b := range extraButtons {
		if pointInRect(x, y, b) {
			return false
		}
	}
	return true
}

// drawSubHeader draws the unified top header for all in-app subviews.
func drawSubHeader(hdc uintptr, title string, hoverBack, hoverClose bool, theme ThemePalette, titleFont, iconFont uintptr) {
	backRect, titleRect, closeRect := subviewCommonHeader()

	drawWindowButton(hdc, backRect, "\uE72B", hoverBack, false, iconFont, theme)

	pSelectObject.Call(hdc, titleFont)
	pSetTextColor.Call(hdc, uintptr(theme.TextPrimary))
	DrawText(hdc, title, &titleRect, DT_SINGLELINE|DT_VCENTER|DT_NOPREFIX)

	drawWindowButton(hdc, closeRect, "\uE8BB", hoverClose, true, iconFont, theme)
}

// --- View 1: Repair View (检查与修复) ---

type repairViewHover struct {
	back    bool
	close   bool
	recheck bool
	done    bool
}

// repairResult 汇总一次接入修复的结果；工作线程只经通道把它交回 UI 线程应用，
// 避免在 goroutine 里直接读写界面状态。
type repairResult struct {
	setupFailed bool
	errors      []string
}

// runRepairCheck 在后台执行耗时的接入修复，完成后投递 WM_USER_REPAIR_DONE。
// 传给工作线程的都是值快照，线程内不触碰 WidgetApp。
func (app *WidgetApp) runRepairCheck(hwnd uintptr) {
	repairSetup := app.repairSetup
	codexConfig := app.paths.CodexConfig
	codexTargets := make([]string, 0, 1)
	for _, status := range app.integrations {
		if status.Fixable && status.Repair == integration.RepairCodexWatch {
			codexTargets = append(codexTargets, status.Name)
		}
	}

	resultCh := make(chan repairResult, 1)
	app.repairDone = resultCh
	go func() {
		executable, _ := os.Executable()
		result := repairResult{}
		if repairSetup != nil {
			ctx, cancel := context.WithTimeout(context.Background(), 2*time.Minute)
			err := repairSetup(ctx)
			cancel()
			if err != nil {
				result.setupFailed = true
				result.errors = append(result.errors, "首次接入："+err.Error())
			}
		}
		for _, name := range codexTargets {
			if err := agent.HandleWatch(codexConfig, executable); err != nil {
				result.errors = append(result.errors, name+"："+err.Error())
			}
		}
		resultCh <- result
		pPostMessageW.Call(hwnd, WM_USER_REPAIR_DONE, 0, 0)
	}()
}

func drawRepairView(hdc uintptr, width, height int32, app *WidgetApp, theme ThemePalette, titleFont, strongFont, baseFont, smallFont, iconFont uintptr) {
	drawSubHeader(hdc, "Agent 接入检查与修复", app.repairHover.back, app.repairHover.close, theme, titleFont, iconFont)

	card := RECT{14, 48, 386, 384}
	fillRoundRect(hdc, card, 8, uintptr(theme.CardBg))
	strokeRoundRect(hdc, card, 8, uintptr(theme.CardBg), uintptr(theme.CardBorder), 1)

	descriptors := agentmeta.All()
	for i, desc := range descriptors {
		rowTop := card.Top + 10 + int32(i)*68
		rowRect := RECT{card.Left + 10, rowTop, card.Right - 10, rowTop + 62}
		fillRoundRect(hdc, rowRect, 6, uintptr(theme.DiagBoxBg))
		strokeRoundRect(hdc, rowRect, 6, uintptr(theme.DiagBoxBg), uintptr(theme.DiagBoxBorder), 1)

		status := app.integrationStatus(desc.ID)
		stateColor := theme.TextMuted
		switch status.State {
		case integration.StateConnected:
			stateColor = theme.AccentSuccess
		case integration.StatePendingRestart:
			stateColor = theme.AccentWarning
		case integration.StateError:
			stateColor = theme.AccentDanger
		}

		// 状态小圆点
		drawEllipseLogical(hdc, rowRect.Left+12, rowRect.Top+14, rowRect.Left+22, rowRect.Top+24, uintptr(stateColor), uintptr(stateColor))

		// Agent 名称
		pSelectObject.Call(hdc, strongFont)
		pSetTextColor.Call(hdc, uintptr(theme.TextPrimary))
		nameRect := RECT{rowRect.Left + 30, rowRect.Top + 6, rowRect.Left + 160, rowRect.Top + 30}
		DrawText(hdc, desc.DisplayName, &nameRect, DT_SINGLELINE|DT_VCENTER|DT_NOPREFIX)

		// 状态标签
		pSelectObject.Call(hdc, smallFont)
		pSetTextColor.Call(hdc, uintptr(stateColor))
		labelRect := RECT{rowRect.Right - 120, rowRect.Top + 6, rowRect.Right - 12, rowRect.Top + 30}
		DrawText(hdc, status.Label(), &labelRect, DT_RIGHT|DT_SINGLELINE|DT_VCENTER|DT_NOPREFIX)

		// 细分界线
		divLine := RECT{rowRect.Left + 10, rowRect.Top + 32, rowRect.Right - 10, rowRect.Top + 33}
		fillRectLogical(hdc, divLine, uintptr(theme.Divider))

		// 描述/行动建议
		descText := status.Detail
		if descText == "" {
			descText = agentHookDescription(desc.ID)
		}
		if status.Action != "" && status.State != integration.StateConnected {
			descText = status.Action
		}
		pSelectObject.Call(hdc, smallFont)
		pSetTextColor.Call(hdc, uintptr(theme.TextSecondary))
		detailRect := RECT{rowRect.Left + 12, rowRect.Top + 35, rowRect.Right - 12, rowRect.Bottom - 4}
		DrawText(hdc, descText, &detailRect, DT_SINGLELINE|DT_VCENTER|DT_END_ELLIPSIS|DT_NOPREFIX)
	}

	// 状态总结或错误提醒
	summaryTop := card.Top + 10 + int32(len(descriptors))*68 + 6
	summaryRect := RECT{card.Left + 12, summaryTop, card.Right - 12, card.Bottom - 8}
	pSelectObject.Call(hdc, smallFont)
	if app.repairing {
		pSetTextColor.Call(hdc, uintptr(theme.AccentWarning))
		DrawText(hdc, "⏳ 正在检查与修复接入配置，请稍候…", &summaryRect, DT_LEFT|DT_WORDBREAK|DT_NOPREFIX)
	} else if app.repairError != "" {
		pSetTextColor.Call(hdc, uintptr(theme.AccentDanger))
		DrawText(hdc, "⚠️ "+app.repairError, &summaryRect, DT_LEFT|DT_WORDBREAK|DT_NOPREFIX)
	} else {
		pSetTextColor.Call(hdc, uintptr(theme.TextMuted))
		DrawText(hdc, "接入检查已完成。绿色表示正常就绪，待重启状态请重启对应终端生效。", &summaryRect, DT_LEFT|DT_WORDBREAK|DT_NOPREFIX)
	}

	// 底部按钮
	recheckBtn := RECT{14, 394, 195, 436}
	doneBtn := RECT{205, 394, 386, 436}
	recheckLabel := "重新检查与修复"
	if app.repairing {
		recheckLabel = "正在修复中…"
	}
	drawIconTextButton(hdc, recheckBtn, "\uE72C", recheckLabel, app.repairHover.recheck, false, false, smallFont, iconFont, theme)
	drawIconTextButton(hdc, doneBtn, "\uE73E", "完成并返回", app.repairHover.done, true, false, smallFont, iconFont, theme)
}

// --- View 2: History View (推送历史) ---

type historyViewHover struct {
	back     bool
	close    bool
	clear    bool
	prev     bool
	next     bool
	copy     bool
	done     bool
	rowIndex int
}

func historyHeaderButtons(total, pageSize int) (prevBtn, nextBtn, clearBtn RECT) {
	clearBtn = RECT{336, 12, 364, 38}
	if total > pageSize {
		clearBtn = RECT{256, 12, 284, 38}
		prevBtn = RECT{288, 12, 314, 38}
		nextBtn = RECT{320, 12, 346, 38}
	}
	return prevBtn, nextBtn, clearBtn
}

// 推送历史列表的行布局：绘制与点击命中共用同一组常量，避免可点范围与可见行不一致。
const (
	historyListPageSize  = 5
	historyListRowInset  = 6
	historyListRowPitch  = 40
	historyListRowHeight = 36
)

// historyRowIndexAt 把列表卡内的坐标换算成可见行号；卡片外、行间空隙或超出本页行数时返回 -1。
func historyRowIndexAt(x, y int32, listCard RECT, pageSize int) int {
	if !pointInRect(x, y, listCard) {
		return -1
	}
	row := int((y - (listCard.Top + historyListRowInset)) / historyListRowPitch)
	if row < 0 || row >= pageSize {
		return -1
	}
	rowTop := listCard.Top + historyListRowInset + int32(row)*historyListRowPitch
	if y >= rowTop+historyListRowHeight {
		return -1
	}
	return row
}

func drawHistoryView(hdc uintptr, width, height int32, app *WidgetApp, theme ThemePalette, titleFont, strongFont, baseFont, smallFont, iconFont uintptr) {
	historyItems, _ := notify.GetHistory(50, app.paths.PushLog)
	total := len(historyItems)
	const pageSize = historyListPageSize
	prevBtn, nextBtn, clearBtn := historyHeaderButtons(total, pageSize)

	backRect, titleRect, closeRect := subviewCommonHeader()
	if total > pageSize {
		titleRect = RECT{48, 12, 190, 38}
	}
	drawWindowButton(hdc, backRect, "\uE72B", app.historyHover.back, false, iconFont, theme)

	pSelectObject.Call(hdc, titleFont)
	pSetTextColor.Call(hdc, uintptr(theme.TextPrimary))
	DrawText(hdc, "推送历史记录", &titleRect, DT_SINGLELINE|DT_VCENTER|DT_NOPREFIX)

	drawWindowButton(hdc, closeRect, "\uE8BB", app.historyHover.close, true, iconFont, theme)

	// 清空按钮与二次确认提示
	if app.historyConfirmClear {
		pSelectObject.Call(hdc, smallFont)
		pSetTextColor.Call(hdc, uintptr(theme.AccentDanger))
		confirmRect := RECT{80, 12, clearBtn.Left - 6, 38}
		DrawText(hdc, "再次点击确认清空", &confirmRect, DT_RIGHT|DT_VCENTER|DT_SINGLELINE|DT_NOPREFIX)
	}
	drawWindowButton(hdc, clearBtn, "\uE74D", app.historyHover.clear, true, iconFont, theme)

	if total > pageSize {
		totalPages := (total + pageSize - 1) / pageSize
		currentPage := (app.historyPageOffset / pageSize) + 1
		pageText := fmt.Sprintf("%d/%d", currentPage, totalPages)
		pSelectObject.Call(hdc, smallFont)
		pSetTextColor.Call(hdc, uintptr(theme.TextMuted))
		pageRect := RECT{190, 12, clearBtn.Left - 6, 38}
		if !app.historyConfirmClear {
			DrawText(hdc, pageText, &pageRect, DT_RIGHT|DT_VCENTER|DT_SINGLELINE|DT_NOPREFIX)
		}
		drawWindowButton(hdc, prevBtn, "\uE76B", app.historyHover.prev, false, iconFont, theme)
		drawWindowButton(hdc, nextBtn, "\uE76C", app.historyHover.next, false, iconFont, theme)
	}

	// 历史列表卡片
	listCard := RECT{14, 48, 386, 260}
	fillRoundRect(hdc, listCard, 8, uintptr(theme.CardBg))
	strokeRoundRect(hdc, listCard, 8, uintptr(theme.CardBg), uintptr(theme.CardBorder), 1)

	start := app.historyPageOffset
	if start >= total && total > 0 {
		start = (total - 1) / pageSize * pageSize
		app.historyPageOffset = start
	}
	end := start + pageSize
	if end > total {
		end = total
	}

	if total == 0 {
		pSelectObject.Call(hdc, baseFont)
		pSetTextColor.Call(hdc, uintptr(theme.TextMuted))
		emptyRect := listCard
		DrawText(hdc, "暂无推送历史记录", &emptyRect, DT_CENTER|DT_VCENTER|DT_SINGLELINE|DT_NOPREFIX)
	} else {
		for i := start; i < end; i++ {
			rowIdx := i - start
			item := historyItems[i]
			top := listCard.Top + historyListRowInset + int32(rowIdx)*historyListRowPitch
			rowRect := RECT{listCard.Left + 6, top, listCard.Right - 6, top + historyListRowHeight}

			fillCol := theme.CardBg
			if app.historySelectedIndex == i {
				fillCol = theme.ButtonBgHover
			} else if app.historyHover.rowIndex == rowIdx {
				fillCol = theme.CardBgHover
			}
			fillRoundRect(hdc, rowRect, 6, uintptr(fillCol))
			if app.historySelectedIndex == i {
				strokeRoundRect(hdc, rowRect, 6, uintptr(fillCol), uintptr(theme.AccentSuccess), 1)
			}

			// Agent 微胶囊
			agentName := historyAgent(item)
			agentBadge := RECT{rowRect.Left + 6, rowRect.Top + 7, rowRect.Left + 62, rowRect.Bottom - 7}
			fillRoundRect(hdc, agentBadge, 4, uintptr(theme.BadgeBg))
			pSelectObject.Call(hdc, smallFont)
			pSetTextColor.Call(hdc, uintptr(theme.TextSecondary))
			DrawText(hdc, agentName, &agentBadge, DT_CENTER|DT_VCENTER|DT_SINGLELINE|DT_NOPREFIX)

			// 时间
			timeText := historyTime(item)
			timeRect := RECT{rowRect.Right - 84, rowRect.Top + 7, rowRect.Right - 6, rowRect.Bottom - 7}
			pSetTextColor.Call(hdc, uintptr(theme.TextMuted))
			DrawText(hdc, timeText, &timeRect, DT_RIGHT|DT_VCENTER|DT_SINGLELINE|DT_NOPREFIX)

			// 标题
			titleColor := theme.TextPrimary
			if item.Status == notify.StatusFailed {
				titleColor = theme.AccentDanger
			}
			pSetTextColor.Call(hdc, uintptr(titleColor))
			itemTitle := cleanRecentPushTitle(item.Title, item.Agent)
			titleRect := RECT{rowRect.Left + 68, rowRect.Top + 7, rowRect.Right - 90, rowRect.Bottom - 7}
			DrawText(hdc, itemTitle, &titleRect, DT_LEFT|DT_VCENTER|DT_SINGLELINE|DT_END_ELLIPSIS|DT_NOPREFIX)
		}
	}

	// 详情预览卡片
	detailCard := RECT{14, 268, 386, 384}
	fillRoundRect(hdc, detailCard, 8, uintptr(theme.CardBg))
	strokeRoundRect(hdc, detailCard, 8, uintptr(theme.CardBg), uintptr(theme.CardBorder), 1)

	if total > 0 && app.historySelectedIndex < total {
		selected := historyItems[app.historySelectedIndex]
		pSelectObject.Call(hdc, strongFont)
		pSetTextColor.Call(hdc, uintptr(theme.TextPrimary))
		dTitleRect := RECT{detailCard.Left + 12, detailCard.Top + 8, detailCard.Right - 12, detailCard.Top + 32}
		DrawText(hdc, selected.Title, &dTitleRect, DT_LEFT|DT_VCENTER|DT_SINGLELINE|DT_END_ELLIPSIS|DT_NOPREFIX)

		pSelectObject.Call(hdc, smallFont)
		pSetTextColor.Call(hdc, uintptr(theme.TextMuted))
		metaText := fmt.Sprintf("%s · %s · %s", historyAgent(selected), selected.Status, selected.Timestamp)
		dMetaRect := RECT{detailCard.Left + 12, detailCard.Top + 32, detailCard.Right - 12, detailCard.Top + 50}
		DrawText(hdc, metaText, &dMetaRect, DT_LEFT|DT_VCENTER|DT_SINGLELINE|DT_NOPREFIX)

		divLine := RECT{detailCard.Left + 10, detailCard.Top + 54, detailCard.Right - 10, detailCard.Top + 55}
		fillRectLogical(hdc, divLine, uintptr(theme.Divider))

		pSetTextColor.Call(hdc, uintptr(theme.TextSecondary))
		dSummaryRect := RECT{detailCard.Left + 12, detailCard.Top + 60, detailCard.Right - 12, detailCard.Bottom - 8}
		summaryContent := selected.Summary
		if strings.TrimSpace(summaryContent) == "" {
			summaryContent = "（无摘要内容）"
		}
		DrawText(hdc, summaryContent, &dSummaryRect, DT_LEFT|DT_WORDBREAK|DT_NOPREFIX)
	} else {
		pSelectObject.Call(hdc, smallFont)
		pSetTextColor.Call(hdc, uintptr(theme.TextMuted))
		dEmptyRect := detailCard
		DrawText(hdc, "选择上方推送条目查看详情", &dEmptyRect, DT_CENTER|DT_VCENTER|DT_SINGLELINE|DT_NOPREFIX)
	}

	// 底部按钮
	copyBtn := RECT{14, 394, 195, 436}
	doneBtn := RECT{205, 394, 386, 436}
	drawIconTextButton(hdc, copyBtn, "\uE8C8", "复制通知内容", app.historyHover.copy, false, false, smallFont, iconFont, theme)
	drawIconTextButton(hdc, doneBtn, "\uE73E", "返回主页", app.historyHover.done, true, false, smallFont, iconFont, theme)
}

// --- View 3: Settings View (系统设置) ---

type settingsViewHover struct {
	back        bool
	close       bool
	relogin     bool
	replyToggle bool
	themePill   bool
	agentCycle  bool
	save        bool
	done        bool
}

func drawSettingsView(hdc uintptr, width, height int32, app *WidgetApp, theme ThemePalette, titleFont, strongFont, baseFont, smallFont, iconFont uintptr) {
	drawSubHeader(hdc, "系统设置", app.settingsHover.back, app.settingsHover.close, theme, titleFont, iconFont)

	// ClawBot 状态卡片
	wechatCard := RECT{14, 48, 386, 104}
	fillRoundRect(hdc, wechatCard, 8, uintptr(theme.CardBg))
	strokeRoundRect(hdc, wechatCard, 8, uintptr(theme.CardBg), uintptr(theme.CardBorder), 1)

	pSelectObject.Call(hdc, iconFont)
	wechatDotCol := theme.AccentSuccess
	wechatText := "ClawBot 微信会话正常"
	if !app.clawbotLoggedIn {
		wechatDotCol = theme.AccentDanger
		wechatText = "ClawBot 微信未登录"
	}
	drawEllipseLogical(hdc, wechatCard.Left+14, wechatCard.Top+22, wechatCard.Left+24, wechatCard.Top+32, uintptr(wechatDotCol), uintptr(wechatDotCol))

	pSelectObject.Call(hdc, strongFont)
	pSetTextColor.Call(hdc, uintptr(theme.TextPrimary))
	wechatLabelRect := RECT{wechatCard.Left + 32, wechatCard.Top + 14, wechatCard.Left + 250, wechatCard.Top + 40}
	DrawText(hdc, wechatText, &wechatLabelRect, DT_LEFT|DT_VCENTER|DT_SINGLELINE|DT_NOPREFIX)

	reloginBtn := RECT{wechatCard.Right - 100, wechatCard.Top + 12, wechatCard.Right - 12, wechatCard.Bottom - 12}
	reloginLabel := "重新扫码"
	if !app.clawbotLoggedIn {
		reloginLabel = "立即登录"
	}
	drawIconTextButton(hdc, reloginBtn, "\uE8BD", reloginLabel, app.settingsHover.relogin, false, false, smallFont, iconFont, theme)

	// 选项卡片
	optCard := RECT{14, 112, 386, 384}
	fillRoundRect(hdc, optCard, 8, uintptr(theme.CardBg))
	strokeRoundRect(hdc, optCard, 8, uintptr(theme.CardBg), uintptr(theme.CardBorder), 1)

	// 1. 静音时段
	pSelectObject.Call(hdc, baseFont)
	pSetTextColor.Call(hdc, uintptr(theme.TextPrimary))
	DrawText(hdc, "静音勿扰时段", &RECT{optCard.Left + 14, optCard.Top + 14, optCard.Left + 160, optCard.Top + 36}, DT_LEFT|DT_VCENTER|DT_SINGLELINE|DT_NOPREFIX)
	pSelectObject.Call(hdc, smallFont)
	if app.settingsError != "" {
		pSetTextColor.Call(hdc, uintptr(theme.AccentDanger))
		DrawText(hdc, "⚠️ "+app.settingsError, &RECT{optCard.Left + 14, optCard.Top + 34, optCard.Left + 236, optCard.Top + 52}, DT_LEFT|DT_VCENTER|DT_SINGLELINE|DT_NOPREFIX)
	} else {
		pSetTextColor.Call(hdc, uintptr(theme.TextMuted))
		DrawText(hdc, "例如 23-8，留空关闭", &RECT{optCard.Left + 14, optCard.Top + 34, optCard.Left + 230, optCard.Top + 52}, DT_LEFT|DT_VCENTER|DT_SINGLELINE|DT_NOPREFIX)
	}

	quietBox := RECT{242, optCard.Top + 10, 376, optCard.Top + 42}
	fillRoundRect(hdc, quietBox, 6, uintptr(theme.InputBg))
	strokeRoundRect(hdc, quietBox, 6, uintptr(theme.InputBg), uintptr(theme.InputBorder), 1)

	// 2. 冷却时间
	div1 := RECT{optCard.Left + 10, optCard.Top + 56, optCard.Right - 10, optCard.Top + 57}
	fillRectLogical(hdc, div1, uintptr(theme.Divider))

	pSelectObject.Call(hdc, baseFont)
	pSetTextColor.Call(hdc, uintptr(theme.TextPrimary))
	DrawText(hdc, "消息去重冷却", &RECT{optCard.Left + 14, optCard.Top + 68, optCard.Left + 160, optCard.Top + 90}, DT_LEFT|DT_VCENTER|DT_SINGLELINE|DT_NOPREFIX)
	pSelectObject.Call(hdc, smallFont)
	pSetTextColor.Call(hdc, uintptr(theme.TextMuted))
	DrawText(hdc, "单位：分钟，默认 10", &RECT{optCard.Left + 14, optCard.Top + 88, optCard.Left + 230, optCard.Top + 106}, DT_LEFT|DT_VCENTER|DT_SINGLELINE|DT_NOPREFIX)

	cooldownBox := RECT{242, optCard.Top + 64, 376, optCard.Top + 96}
	fillRoundRect(hdc, cooldownBox, 6, uintptr(theme.InputBg))
	strokeRoundRect(hdc, cooldownBox, 6, uintptr(theme.InputBg), uintptr(theme.InputBorder), 1)

	// 3. 引用回复开关
	div2 := RECT{optCard.Left + 10, optCard.Top + 118, optCard.Right - 10, optCard.Top + 119}
	fillRectLogical(hdc, div2, uintptr(theme.Divider))

	pSelectObject.Call(hdc, baseFont)
	pSetTextColor.Call(hdc, uintptr(theme.TextPrimary))
	DrawText(hdc, "微信引用回复续聊", &RECT{optCard.Left + 14, optCard.Top + 130, optCard.Left + 200, optCard.Top + 152}, DT_LEFT|DT_VCENTER|DT_SINGLELINE|DT_NOPREFIX)
	pSelectObject.Call(hdc, smallFont)
	pSetTextColor.Call(hdc, uintptr(theme.TextMuted))
	DrawText(hdc, "引用推送消息可续聊对应任务", &RECT{optCard.Left + 14, optCard.Top + 150, optCard.Left + 260, optCard.Top + 168}, DT_LEFT|DT_VCENTER|DT_SINGLELINE|DT_NOPREFIX)

	// Switch 开关
	replyTrack := RECT{optCard.Right - 64, optCard.Top + 134, optCard.Right - 18, optCard.Top + 158}
	trackCol := theme.SwitchTrackOff
	if app.replyEnabled {
		trackCol = theme.SwitchTrackOn
	}
	fillRoundRect(hdc, replyTrack, 10, uintptr(trackCol))
	knobX := replyTrack.Left + 3
	if app.replyEnabled {
		knobX = replyTrack.Right - 19
	}
	drawEllipseLogical(hdc, knobX, replyTrack.Top+3, knobX+16, replyTrack.Bottom-3, uintptr(theme.KnobColor), uintptr(theme.KnobColor))

	// 4. 界面主题
	div3 := RECT{optCard.Left + 10, optCard.Top + 176, optCard.Right - 10, optCard.Top + 177}
	fillRectLogical(hdc, div3, uintptr(theme.Divider))

	pSelectObject.Call(hdc, baseFont)
	pSetTextColor.Call(hdc, uintptr(theme.TextPrimary))
	DrawText(hdc, "界面主题模式", &RECT{optCard.Left + 14, optCard.Top + 188, optCard.Left + 200, optCard.Top + 210}, DT_LEFT|DT_VCENTER|DT_SINGLELINE|DT_NOPREFIX)
	pSelectObject.Call(hdc, smallFont)
	pSetTextColor.Call(hdc, uintptr(theme.TextMuted))
	DrawText(hdc, "支持白天与夜晚模式切换", &RECT{optCard.Left + 14, optCard.Top + 208, optCard.Left + 230, optCard.Top + 226}, DT_LEFT|DT_VCENTER|DT_SINGLELINE|DT_NOPREFIX)

	themePill := RECT{optCard.Right - 100, optCard.Top + 188, optCard.Right - 18, optCard.Top + 220}
	themeLabel := "夜晚 🌙"
	if app.theme == "light" {
		themeLabel = "白天 ☀️"
	}
	drawIconTextButton(hdc, themePill, "", themeLabel, app.settingsHover.themePill, false, false, smallFont, iconFont, theme)

	// 5. 默认代理
	div4 := RECT{optCard.Left + 10, optCard.Top + 234, optCard.Right - 10, optCard.Top + 235}
	fillRectLogical(hdc, div4, uintptr(theme.Divider))

	pSelectObject.Call(hdc, baseFont)
	pSetTextColor.Call(hdc, uintptr(theme.TextPrimary))
	DrawText(hdc, "默认聚焦代理", &RECT{optCard.Left + 14, optCard.Top + 244, optCard.Left + 200, optCard.Top + 266}, DT_LEFT|DT_VCENTER|DT_SINGLELINE|DT_NOPREFIX)

	agentPill := RECT{optCard.Right - 120, optCard.Top + 240, optCard.Right - 18, optCard.Top + 270}
	defaultName := "Antigravity"
	if desc, ok := agentmeta.Lookup(app.currentAgent); ok {
		defaultName = desc.DisplayName
	}
	drawIconTextButton(hdc, agentPill, "", defaultName+" ▾", app.settingsHover.agentCycle, false, false, smallFont, iconFont, theme)

	// 底部保存/返回
	saveBtn := RECT{14, 394, 195, 436}
	doneBtn := RECT{205, 394, 386, 436}
	drawIconTextButton(hdc, saveBtn, "\uE74E", "保存设置", app.settingsHover.save, true, false, smallFont, iconFont, theme)
	drawIconTextButton(hdc, doneBtn, "\uE73E", "返回主页", app.settingsHover.done, false, false, smallFont, iconFont, theme)
}

// --- View 4: Login View (微信配置/扫码) ---

type loginViewHover struct {
	back    bool
	close   bool
	refresh bool
	done    bool
	submit  bool
}

// loginVerifyRects 返回扫码登录索要数字配对码时的输入框与提交按钮位置（逻辑坐标）。
func loginVerifyRects() (field, submit RECT) {
	return RECT{110, 268, 290, 302}, RECT{130, 310, 270, 342}
}

// verifyEditRect 返回配对码 EDIT 控件的实际位置（已按 DPI 缩放）。
func verifyEditRect() RECT {
	field, _ := loginVerifyRects()
	return scaleRect(RECT{field.Left + 6, field.Top + 4, field.Right - 6, field.Bottom - 4})
}

func drawLoginView(hdc uintptr, width, height int32, app *WidgetApp, theme ThemePalette, titleFont, strongFont, baseFont, smallFont, iconFont uintptr) {
	drawSubHeader(hdc, "微信扫码配置", app.loginHover.back, app.loginHover.close, theme, titleFont, iconFont)

	card := RECT{14, 48, 386, 384}
	fillRoundRect(hdc, card, 8, uintptr(theme.CardBg))
	strokeRoundRect(hdc, card, 8, uintptr(theme.CardBg), uintptr(theme.CardBorder), 1)

	_, bitmap, status, _, success, promptActive := app.loginState.snapshot()

	if app.clawbotLoggedIn || success {
		// 已登录成功状态
		pSelectObject.Call(hdc, iconFont)
		pSetTextColor.Call(hdc, uintptr(theme.AccentSuccess))
		checkIconRect := RECT{card.Left + 14, card.Top + 60, card.Right - 14, card.Top + 140}
		DrawText(hdc, "\uE73E", &checkIconRect, DT_CENTER|DT_VCENTER|DT_SINGLELINE|DT_NOPREFIX)

		pSelectObject.Call(hdc, strongFont)
		pSetTextColor.Call(hdc, uintptr(theme.TextPrimary))
		sTitleRect := RECT{card.Left + 14, card.Top + 150, card.Right - 14, card.Top + 180}
		DrawText(hdc, "ClawBot 微信已成功连接", &sTitleRect, DT_CENTER|DT_VCENTER|DT_SINGLELINE|DT_NOPREFIX)

		pSelectObject.Call(hdc, smallFont)
		pSetTextColor.Call(hdc, uintptr(theme.TextSecondary))
		sHintRect := RECT{card.Left + 24, card.Top + 190, card.Right - 24, card.Top + 240}
		DrawText(hdc, "主动推送链路正常。任务完成后将自动通过微信发送消息。", &sHintRect, DT_CENTER|DT_WORDBREAK|DT_NOPREFIX)
	} else {
		// 二维码区域 (180x180 居中)
		qrBox := RECT{110, 68, 290, 248}
		if len(bitmap) > 0 {
			drawQRCode(hdc, qrBox, bitmap)
		} else {
			fillRoundRect(hdc, qrBox, 8, uintptr(theme.DiagBoxBg))
			strokeRoundRect(hdc, qrBox, 8, uintptr(theme.DiagBoxBg), uintptr(theme.DiagBoxBorder), 1)
			pSelectObject.Call(hdc, smallFont)
			pSetTextColor.Call(hdc, uintptr(theme.TextMuted))
			pStatus := status
			if pStatus == "" {
				pStatus = "正在获取二维码…"
			}
			DrawText(hdc, pStatus, &qrBox, DT_CENTER|DT_VCENTER|DT_WORDBREAK|DT_NOPREFIX)
		}

		// 状态提示
		pSelectObject.Call(hdc, strongFont)
		pSetTextColor.Call(hdc, uintptr(theme.TextPrimary))
		statusTop := card.Top + 260
		if promptActive {
			// 配对码输入框占据卡片中部，状态文案顺延到输入框下方
			statusTop = card.Top + 300
		}
		promptRect := RECT{card.Left + 14, statusTop, card.Right - 14, statusTop + 28}
		statusText := "请使用手机微信扫码"
		if status != "" {
			statusText = status
		}
		DrawText(hdc, statusText, &promptRect, DT_CENTER|DT_VCENTER|DT_SINGLELINE|DT_NOPREFIX)

		if promptActive {
			// 微信要求数字配对码：EDIT 控件由原生窗口渲染，这里只画底框与提交按钮
			field, submitBtn := loginVerifyRects()
			fillRoundRect(hdc, field, 6, uintptr(theme.InputBg))
			strokeRoundRect(hdc, field, 6, uintptr(theme.InputBg), uintptr(theme.InputBorder), 1)
			drawIconTextButton(hdc, submitBtn, "\uE73E", "提交配对码", app.loginHover.submit, true, false, smallFont, iconFont, theme)
		} else {
			pSelectObject.Call(hdc, smallFont)
			pSetTextColor.Call(hdc, uintptr(theme.TextMuted))
			instructionRect := RECT{card.Left + 24, card.Top + 294, card.Right - 24, card.Bottom - 6}
			DrawText(hdc, "扫码登录后，请在微信中向 ClawBot 发送任意一条消息以建立主动推送会话。", &instructionRect, DT_CENTER|DT_WORDBREAK|DT_NOPREFIX)
		}
	}

	// 底部按钮
	refreshBtn := RECT{14, 394, 195, 436}
	doneBtn := RECT{205, 394, 386, 436}
	drawIconTextButton(hdc, refreshBtn, "\uE72C", "刷新二维码", app.loginHover.refresh, false, false, smallFont, iconFont, theme)
	drawIconTextButton(hdc, doneBtn, "\uE73E", "完成并返回", app.loginHover.done, true, false, smallFont, iconFont, theme)
}

// --- Custom Fluent Dropdown Overlay ---

func drawAgentDropdownOverlay(hdc uintptr, app *WidgetApp, theme ThemePalette, strongFont, smallFont, iconFont uintptr) {
	// 下拉框紧贴在 singleAgent 模式的 switchAgent (76, 92, 256, 128) 正下方
	dropRect := RECT{76, 130, 266, 272}
	fillRoundRect(hdc, dropRect, 8, uintptr(theme.DropdownBg))
	strokeRoundRect(hdc, dropRect, 8, uintptr(theme.DropdownBg), uintptr(theme.DropdownBorder), 1)

	descriptors := agentmeta.All()
	for i, desc := range descriptors {
		itemTop := dropRect.Top + 4 + int32(i)*34
		itemRect := RECT{dropRect.Left + 4, itemTop, dropRect.Right - 4, itemTop + 32}

		if app.dropdownHoverIndex == i {
			fillRoundRect(hdc, itemRect, 6, uintptr(theme.DropdownHover))
		}

		// 状态圆点
		status := app.integrationStatus(desc.ID)
		stateColor := theme.TextMuted
		switch status.State {
		case integration.StateConnected:
			stateColor = theme.AccentSuccess
		case integration.StatePendingRestart:
			stateColor = theme.AccentWarning
		case integration.StateError:
			stateColor = theme.AccentDanger
		}
		drawEllipseLogical(hdc, itemRect.Left+10, itemRect.Top+11, itemRect.Left+18, itemRect.Top+19, uintptr(stateColor), uintptr(stateColor))

		// 名称
		pSelectObject.Call(hdc, strongFont)
		nameColor := theme.TextPrimary
		if desc.ID == app.focusedAgentID() {
			nameColor = theme.AccentSuccess
		}
		pSetTextColor.Call(hdc, uintptr(nameColor))
		nameRect := RECT{itemRect.Left + 26, itemRect.Top, itemRect.Right - 48, itemRect.Bottom}
		DrawText(hdc, desc.DisplayName, &nameRect, DT_SINGLELINE|DT_VCENTER|DT_NOPREFIX)

		// 选中打勾或状态简标
		pSelectObject.Call(hdc, smallFont)
		if desc.ID == app.focusedAgentID() {
			pSelectObject.Call(hdc, iconFont)
			pSetTextColor.Call(hdc, uintptr(theme.AccentSuccess))
			checkRect := RECT{itemRect.Right - 28, itemRect.Top, itemRect.Right - 6, itemRect.Bottom}
			DrawText(hdc, "\uE73E", &checkRect, DT_CENTER|DT_VCENTER|DT_SINGLELINE|DT_NOPREFIX)
		} else {
			pSetTextColor.Call(hdc, uintptr(theme.TextMuted))
			subLabelRect := RECT{itemRect.Right - 60, itemRect.Top, itemRect.Right - 6, itemRect.Bottom}
			DrawText(hdc, status.Label(), &subLabelRect, DT_RIGHT|DT_VCENTER|DT_SINGLELINE|DT_NOPREFIX)
		}
	}
}
