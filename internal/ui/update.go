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
	// WM_USER_UPDATE_AVAILABLE 由后台静默检查在发现新版本后投递，只重绘按钮，不弹窗。
	WM_USER_UPDATE_AVAILABLE = WM_USER + 6

	updateCheckTimeout = 25 * time.Second
	updatePrepareTime  = 4 * time.Minute

	// 后台静默检查：启动后延迟一次，随后每 2 小时轮询。发现新版本只把"升级"按钮标黄，
	// 不弹窗；失败只写调试日志。
	updateBackgroundInitialDelay = 15 * time.Second
	updateBackgroundInterval     = 2 * time.Hour
)

type widgetUpdateState struct {
	mu         sync.Mutex
	busy       string // 用户可见的忙状态："checking"/"installing"，会影响按钮文案
	background bool   // 后台静默检查进行中；不改变按钮文案，仅用于互斥
	release    *update.Release
	errText    string
}

// newUpdateClient 便于测试注入更新客户端；默认走生产客户端。
var newUpdateClient = update.NewClient

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
		release, available, err := newUpdateClient().Check(ctx, app.Version())
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
	case WM_USER_UPDATE_AVAILABLE:
		// 后台发现新版本：只重绘，把"升级"按钮标黄，不打断用户。
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
			if prepared.Kind == update.ArtifactInstaller {
				err = prepared.Launch(logPath, app.updateSetupArgs()...)
			} else {
				err = prepared.Launch(logPath, app.updateInstallerArgs()...)
			}
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
	arguments := []string{"-SkipLoginLaunch"}
	if installDir := currentInstallDir(); installDir != "" {
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

// updateSetupArgs 把当前安装目录交给标准安装器：升级必须原地进行，
// 不允许因为安装器的默认目录而搬到 %LOCALAPPDATA%\Programs\Agent-notify。
func (app *WidgetApp) updateSetupArgs() []string {
	installDir := currentInstallDir()
	if installDir == "" {
		return nil
	}
	return []string{"/DIR=" + installDir}
}

// currentInstallDir 返回正在运行的 exe 所在目录，升级时用它锁定安装位置。
func currentInstallDir() string {
	executable, err := os.Executable()
	if err != nil {
		return ""
	}
	installDir := strings.TrimSpace(filepath.Dir(executable))
	if installDir == "" || installDir == "." {
		return ""
	}
	return installDir
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

// beginBackgroundUpdateCheck 占用后台检查标志；不改变用户可见的 busy 状态，也不清空已发现的新版本。
func (app *WidgetApp) beginBackgroundUpdateCheck() bool {
	app.updateState.mu.Lock()
	defer app.updateState.mu.Unlock()
	if app.updateState.busy != "" || app.updateState.background {
		return false
	}
	app.updateState.background = true
	return true
}

// finishBackgroundUpdateCheck 结束后台检查；release 为 nil（失败或已是最新）时保留已发现的新版本。
func (app *WidgetApp) finishBackgroundUpdateCheck(release *update.Release) {
	app.updateState.mu.Lock()
	defer app.updateState.mu.Unlock()
	app.updateState.background = false
	if release != nil {
		app.updateState.release = release
	}
}

// checkUpdateSilently 做一次静默检查：有新版本就更新按钮状态并通知重绘，失败只写调试日志。
func (app *WidgetApp) checkUpdateSilently() {
	if !app.beginBackgroundUpdateCheck() {
		return
	}
	ctx, cancel := context.WithTimeout(context.Background(), updateCheckTimeout)
	defer cancel()
	release, available, err := newUpdateClient().Check(ctx, app.Version())
	if err != nil {
		debugLog("background update check: %v", err)
		app.finishBackgroundUpdateCheck(nil)
		return
	}
	if !available {
		app.finishBackgroundUpdateCheck(nil)
		return
	}
	app.finishBackgroundUpdateCheck(&release)
	if hwnd := app.window(); hwnd != 0 {
		pPostMessageW.Call(hwnd, WM_USER_UPDATE_AVAILABLE, 0, 0)
	}
}

// runBackgroundUpdateChecks 启动延迟后做一次静默检查，之后每 updateBackgroundInterval 轮询，
// 直到 ctx 结束（悬浮窗退出）。
func (app *WidgetApp) runBackgroundUpdateChecks(ctx context.Context) {
	timer := time.NewTimer(updateBackgroundInitialDelay)
	defer timer.Stop()
	for {
		select {
		case <-ctx.Done():
			return
		case <-timer.C:
		}
		app.checkUpdateSilently()
		timer.Reset(updateBackgroundInterval)
	}
}
