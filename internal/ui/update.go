//go:build windows

package ui

import (
	"context"
	"fmt"
	"os"
	"path/filepath"
	"strings"
	"sync"
	"time"

	"github.com/srafyhucl-cpu/agent-notify/internal/update"
)

const (
	WM_USER_UPDATE_READY   = WM_USER + 2
	WM_USER_UPDATE_LATEST  = WM_USER + 3
	WM_USER_UPDATE_ERROR   = WM_USER + 4
	WM_USER_UPDATE_RESTART = WM_USER + 5

	updateCheckTimeout = 25 * time.Second
	updatePrepareTime  = 4 * time.Minute
)

type widgetUpdateState struct {
	mu      sync.Mutex
	busy    string
	release *update.Release
	errText string
}

func (app *WidgetApp) handleUpdateClick(hwnd uintptr) {
	if release, ok := app.pendingUpdate(); ok {
		app.confirmUpdate(hwnd, release)
		return
	}
	app.startUpdateCheck(hwnd)
}

func (app *WidgetApp) startUpdateCheck(hwnd uintptr) {
	if !app.beginUpdateCheck() {
		return
	}
	pInvalidateRect.Call(hwnd, 0, 0)

	go func() {
		ctx, cancel := context.WithTimeout(context.Background(), updateCheckTimeout)
		defer cancel()
		release, available, err := update.NewClient().Check(ctx, app.Version())
		if err != nil {
			app.finishUpdateCheck(nil, err.Error())
			pPostMessageW.Call(hwnd, WM_USER_UPDATE_ERROR, 0, 0)
			return
		}
		if !available {
			app.finishUpdateCheck(nil, "")
			pPostMessageW.Call(hwnd, WM_USER_UPDATE_LATEST, 0, 0)
			return
		}
		app.finishUpdateCheck(&release, "")
		pPostMessageW.Call(hwnd, WM_USER_UPDATE_READY, 0, 0)
	}()
}

func (app *WidgetApp) handleUpdateMessage(hwnd uintptr, message uint32) bool {
	switch message {
	case WM_USER_UPDATE_READY:
		if release, ok := app.pendingUpdate(); ok {
			app.confirmUpdate(hwnd, release)
		}
		return true
	case WM_USER_UPDATE_LATEST:
		showMessage(hwnd, "当前已是最新版本。", MB_ICONINFO)
		pInvalidateRect.Call(hwnd, 0, 0)
		return true
	case WM_USER_UPDATE_ERROR:
		detail := app.takeUpdateError()
		if detail == "" {
			detail = "未知错误"
		}
		showMessage(hwnd, "检查或安装更新失败：\n"+detail, MB_ICONINFO)
		pInvalidateRect.Call(hwnd, 0, 0)
		return true
	case WM_USER_UPDATE_RESTART:
		savePosition(hwnd, app.paths.WidgetPosFile)
		pDestroyWindow.Call(hwnd)
		return true
	default:
		return false
	}
}

func (app *WidgetApp) confirmUpdate(hwnd uintptr, release update.Release) {
	message := fmt.Sprintf(
		"发现新版本 v%s。\n\n是否立即下载并安装？安装完成后会自动重启悬浮窗，不会重启正在运行的 Agent。",
		release.Version,
	)
	if !showConfirm(hwnd, message) {
		return
	}
	app.startUpdateInstall(hwnd, release)
}

func (app *WidgetApp) startUpdateInstall(hwnd uintptr, release update.Release) {
	if !app.beginUpdateInstall() {
		return
	}
	pInvalidateRect.Call(hwnd, 0, 0)

	go func() {
		ctx, cancel := context.WithTimeout(context.Background(), updatePrepareTime)
		defer cancel()
		root := filepath.Join(app.paths.TempDir, "updates")
		prepared, err := update.NewClient().Prepare(ctx, release, root)
		if err == nil {
			logPath := filepath.Join(root, "last-update.log")
			err = prepared.Launch(logPath, app.updateInstallerArgs()...)
		}
		if err != nil {
			app.finishUpdateInstall(err.Error())
			pPostMessageW.Call(hwnd, WM_USER_UPDATE_ERROR, 0, 0)
			return
		}
		pPostMessageW.Call(hwnd, WM_USER_UPDATE_RESTART, 0, 0)
	}()
}

func (app *WidgetApp) updateInstallerArgs() []string {
	executable, _ := os.Executable()
	arguments := []string{"-SkipLoginLaunch"}
	if installDir := strings.TrimSpace(filepath.Dir(executable)); installDir != "" && installDir != "." {
		arguments = append(arguments, "-InstallDir", installDir)
	}
	if pluginDir := strings.TrimSpace(filepath.Dir(app.paths.PluginFile)); pluginDir != "" && pluginDir != "." {
		arguments = append(arguments, "-PluginDir", pluginDir)
	}
	if path := strings.TrimSpace(app.paths.DevinExtensionDir); path != "" {
		arguments = append(arguments, "-DevinExtensionDir", path)
	}
	if path := strings.TrimSpace(app.paths.CodexConfig); path != "" {
		arguments = append(arguments, "-CodexConfig", path)
	}
	if path := strings.TrimSpace(app.paths.AntigravityHooks); path != "" {
		arguments = append(arguments, "-AntigravityHooks", path)
	}
	if path := strings.TrimSpace(app.paths.DevinConfig); path != "" {
		arguments = append(arguments, "-DevinConfig", path)
	}
	return arguments
}

func (app *WidgetApp) updateButtonState() (string, bool) {
	app.updateState.mu.Lock()
	defer app.updateState.mu.Unlock()
	switch app.updateState.busy {
	case "checking":
		return "检查中", false
	case "installing":
		return "升级中", false
	}
	if app.updateState.release != nil {
		return "升级 v" + app.updateState.release.Version, true
	}
	return "升级", false
}

func (app *WidgetApp) beginUpdateCheck() bool {
	app.updateState.mu.Lock()
	defer app.updateState.mu.Unlock()
	if app.updateState.busy != "" {
		return false
	}
	app.updateState.busy = "checking"
	app.updateState.release = nil
	app.updateState.errText = ""
	return true
}

func (app *WidgetApp) finishUpdateCheck(release *update.Release, errText string) {
	app.updateState.mu.Lock()
	defer app.updateState.mu.Unlock()
	app.updateState.busy = ""
	app.updateState.release = release
	app.updateState.errText = errText
}

func (app *WidgetApp) beginUpdateInstall() bool {
	app.updateState.mu.Lock()
	defer app.updateState.mu.Unlock()
	if app.updateState.busy != "" || app.updateState.release == nil {
		return false
	}
	app.updateState.busy = "installing"
	app.updateState.errText = ""
	return true
}

func (app *WidgetApp) finishUpdateInstall(errText string) {
	app.updateState.mu.Lock()
	defer app.updateState.mu.Unlock()
	app.updateState.busy = ""
	app.updateState.errText = errText
}

func (app *WidgetApp) pendingUpdate() (update.Release, bool) {
	app.updateState.mu.Lock()
	defer app.updateState.mu.Unlock()
	if app.updateState.release == nil {
		return update.Release{}, false
	}
	return *app.updateState.release, true
}

func (app *WidgetApp) takeUpdateError() string {
	app.updateState.mu.Lock()
	defer app.updateState.mu.Unlock()
	text := app.updateState.errText
	app.updateState.errText = ""
	return text
}
