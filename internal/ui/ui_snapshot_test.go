//go:build windows

package ui

import (
	"encoding/binary"
	"image"
	"image/color"
	"image/png"
	"os"
	"path/filepath"
	"testing"
	"unsafe"

	"github.com/srafyhucl-cpu/agent-notify/internal/agentmeta"
	"github.com/srafyhucl-cpu/agent-notify/internal/config"
	"github.com/srafyhucl-cpu/agent-notify/internal/integration"
)

// 渲染快照脚手架：把 drawUI 的结果直接画进 DIB 并落成 PNG，用于人工目视复核与回归。
// 默认跳过，设置 AGENT_NOTIFY_SNAPSHOT_DIR 后运行：
//
//	$env:AGENT_NOTIFY_SNAPSHOT_DIR = 'D:\Temp\ui-shots'
//	go test ./internal/ui/ -run TestUISnapshot
func snapshotApp() *WidgetApp {
	connected := func(agent string) integration.Status {
		return integration.Status{Agent: agent, Enabled: true, State: integration.StateConnected, Detail: "已接入"}
	}
	return &WidgetApp{
		paths:                config.GetPaths(),
		theme:                "dark",
		agentMode:            "grid",
		currentAgent:         agentmeta.Antigravity,
		onOpenCode:           true,
		onCodex:              true,
		onAntigravity:        true,
		onDevin:              true,
		onCommandCode:        true,
		clawbotLoggedIn:      true,
		clawbotSessionReady:  true,
		quietHours:           "",
		replyEnabled:         true,
		cooldownMin:          10,
		commandCodeWindowSec: 60,
		lastPushTitle:        "**🟢 CommandCode｜Greeting Session**",
		lastPushSummary:      "Hi — what are you working on?",
		lastPushStatus:       "成功",
		lastPushAgent:        "commandcode",
		integrations: map[string]integration.Status{
			agentmeta.OpenCode:    connected(agentmeta.OpenCode),
			agentmeta.Codex:       connected(agentmeta.Codex),
			agentmeta.Antigravity: connected(agentmeta.Antigravity),
			agentmeta.Devin:       connected(agentmeta.Devin),
			agentmeta.CommandCode: connected(agentmeta.CommandCode),
		},
	}
}

func renderSnapshot(t *testing.T, name string, view WidgetView) {
	t.Helper()
	outDir := os.Getenv("AGENT_NOTIFY_SNAPSHOT_DIR")
	if outDir == "" {
		t.Skip("AGENT_NOTIFY_SNAPSHOT_DIR not set")
	}
	if err := os.MkdirAll(outDir, 0o755); err != nil {
		t.Fatal(err)
	}

	app := snapshotApp()
	app.currentView = view

	width, height := widgetWidth, widgetHeight
	hdcScreen, _, _ := pGetDC.Call(0)
	if hdcScreen == 0 {
		t.Fatal("GetDC failed")
	}
	defer pReleaseDC.Call(0, hdcScreen)

	hdcMem, _, _ := pCreateCompatibleDC.Call(hdcScreen)
	if hdcMem == 0 {
		t.Fatal("CreateCompatibleDC failed")
	}
	defer pDeleteDC.Call(hdcMem)

	var info [40]byte
	binary.LittleEndian.PutUint32(info[0:], 40)
	binary.LittleEndian.PutUint32(info[4:], uint32(width))
	binary.LittleEndian.PutUint32(info[8:], uint32(int32(-height))) // 负高度 = 自上而下
	binary.LittleEndian.PutUint16(info[12:], 1)
	binary.LittleEndian.PutUint16(info[14:], 32)

	var bits unsafe.Pointer
	hBitmap, _, _ := pCreateDIBSection.Call(
		hdcScreen,
		uintptr(unsafe.Pointer(&info[0])),
		0,
		uintptr(unsafe.Pointer(&bits)),
		0, 0,
	)
	if hBitmap == 0 || bits == nil {
		t.Fatal("CreateDIBSection failed")
	}
	defer pDeleteObject.Call(hBitmap)

	previous, _, _ := pSelectObject.Call(hdcMem, hBitmap)
	drawUI(hdcMem, width, height, app)
	pSelectObject.Call(hdcMem, previous)

	pixels := unsafe.Slice((*byte)(bits), int(width)*int(height)*4)
	img := image.NewRGBA(image.Rect(0, 0, int(width), int(height)))
	for i := 0; i < int(width)*int(height); i++ {
		b, g, r := pixels[i*4], pixels[i*4+1], pixels[i*4+2]
		img.SetRGBA(i%int(width), i/int(width), color.RGBA{R: r, G: g, B: b, A: 255})
	}

	path := filepath.Join(outDir, name+".png")
	file, err := os.Create(path)
	if err != nil {
		t.Fatal(err)
	}
	defer file.Close()
	if err := png.Encode(file, img); err != nil {
		t.Fatal(err)
	}
	t.Logf("snapshot: %s", path)
}

func TestUISnapshotDashboard(t *testing.T) {
	renderSnapshot(t, "dashboard", WidgetViewDashboard)
}

func TestUISnapshotRepair(t *testing.T) {
	renderSnapshot(t, "repair", WidgetViewRepair)
}

func TestUISnapshotHistory(t *testing.T) {
	renderSnapshot(t, "history", WidgetViewHistory)
}

func TestUISnapshotSettings(t *testing.T) {
	renderSnapshot(t, "settings", WidgetViewSettings)
}

func TestUISnapshotLogin(t *testing.T) {
	renderSnapshot(t, "login", WidgetViewLogin)
}
