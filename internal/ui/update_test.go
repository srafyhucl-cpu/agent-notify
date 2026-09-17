//go:build windows

package ui

import (
	"fmt"
	"net/http"
	"net/http/httptest"
	"os"
	"path/filepath"
	"testing"

	"github.com/srafyhucl-cpu/agent-notify/internal/config"
	"github.com/srafyhucl-cpu/agent-notify/internal/update"
)

func TestUpdateButtonStateTransitions(t *testing.T) {
	app := &WidgetApp{}
	if label, primary := app.updateButtonState(); label != "升级" || primary {
		t.Fatalf("initial update state = %q primary=%v", label, primary)
	}
	if !app.beginUpdateCheck() {
		t.Fatal("beginUpdateCheck rejected the first check")
	}
	if label, primary := app.updateButtonState(); label != "检查中" || primary {
		t.Fatalf("checking state = %q primary=%v", label, primary)
	}
	app.finishUpdateCheck(nil, "")
	if label, primary := app.updateButtonState(); label != "升级" || primary {
		t.Fatalf("no-update state = %q primary=%v", label, primary)
	}
	if !app.beginUpdateCheck() {
		t.Fatal("beginUpdateCheck rejected the second check")
	}
	release := update.Release{Version: "1.4.0"}
	app.finishUpdateCheck(&release, "")
	if label, primary := app.updateButtonState(); label != "升级 v1.4.0" || !primary {
		t.Fatalf("available state = %q primary=%v", label, primary)
	}
	if !app.beginUpdateInstall() {
		t.Fatal("beginUpdateInstall rejected an available update")
	}
	if label, primary := app.updateButtonState(); label != "升级中" || primary {
		t.Fatalf("installing state = %q primary=%v", label, primary)
	}
	app.finishUpdateInstall("download failed")
	if label, primary := app.updateButtonState(); label != "升级 v1.4.0" || !primary {
		t.Fatalf("failed install state = %q primary=%v", label, primary)
	}
	if got := app.takeUpdateError(); got != "download failed" {
		t.Fatalf("update error = %q", got)
	}
}

func TestUpdateInstallerArgsPreserveConfiguredPaths(t *testing.T) {
	app := &WidgetApp{
		paths: config.Paths{
			PluginFile:        `D:\Tools\opencode\agent-notify.ts`,
			DevinExtensionDir: `D:\Tools\devin-extension`,
			CodexConfig:       `D:\Tools\codex\config.toml`,
			AntigravityHooks:  `D:\Tools\antigravity\hooks.json`,
			DevinConfig:       `D:\Tools\devin\config.json`,
		},
	}
	arguments := app.updateInstallerArgs()
	want := map[string]string{
		"-PluginDir":         filepath.Dir(app.paths.PluginFile),
		"-DevinExtensionDir": app.paths.DevinExtensionDir,
		"-CodexConfig":       app.paths.CodexConfig,
		"-AntigravityHooks":  app.paths.AntigravityHooks,
		"-DevinConfig":       app.paths.DevinConfig,
	}
	for index := 0; index < len(arguments); index++ {
		if arguments[index] == "-PluginDir" || arguments[index] == "-DevinExtensionDir" ||
			arguments[index] == "-CodexConfig" || arguments[index] == "-AntigravityHooks" ||
			arguments[index] == "-DevinConfig" {
			expected, ok := want[arguments[index]]
			if !ok || index+1 >= len(arguments) || arguments[index+1] != expected {
				t.Fatalf("argument %q = %q, want %q", arguments[index], arguments[index+1], expected)
			}
		}
	}
	if !containsString(arguments, "-InstallDir") || !containsString(arguments, "-SkipLoginLaunch") {
		t.Fatalf("arguments = %#v, missing install dir or login suppression", arguments)
	}
}

func TestUpdateSetupArgsLockInstallDir(t *testing.T) {
	app := &WidgetApp{}
	executable, err := os.Executable()
	if err != nil {
		t.Fatal(err)
	}
	want := "/DIR=" + filepath.Dir(executable)
	arguments := app.updateSetupArgs()
	if len(arguments) != 1 || arguments[0] != want {
		t.Fatalf("updateSetupArgs = %#v, want %q", arguments, want)
	}
}

func containsString(values []string, expected string) bool {
	for _, value := range values {
		if value == expected {
			return true
		}
	}
	return false
}

// 后台静默检查的并发约束：不重复进入、不与手动检查/安装重叠、失败不抹掉已发现的新版本。
func TestBackgroundUpdateCheckStateGuards(t *testing.T) {
	app := &WidgetApp{}
	known := update.Release{Version: "9.9.9"}
	app.updateState.release = &known

	if !app.beginBackgroundUpdateCheck() {
		t.Fatal("first background check should start")
	}
	if app.beginBackgroundUpdateCheck() {
		t.Fatal("overlapping background check should be rejected")
	}
	// 后台检查失败（release=nil）不得抹掉已发现的新版本。
	app.finishBackgroundUpdateCheck(nil)
	if release, ok := app.pendingUpdate(); !ok || release.Version != "9.9.9" {
		t.Fatalf("known release lost after background check: %+v ok=%v", release, ok)
	}

	// 手动检查进行中时，后台检查跳过，避免与用户可见状态互相干扰。
	if !app.beginUpdateCheck() {
		t.Fatal("manual check should start")
	}
	if app.beginBackgroundUpdateCheck() {
		t.Fatal("background check should skip while a manual check is in flight")
	}
	app.finishUpdateCheck(nil, "")
}

// 静默检查发现新版本时写入状态（按钮由此标黄），且不设置用户可见的忙状态。
func TestCheckUpdateSilentlyStoresRelease(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.URL.Path != "/repos/owner/repo/releases/latest" {
			http.NotFound(w, r)
			return
		}
		base := "http://" + r.Host
		w.Header().Set("Content-Type", "application/json")
		_, _ = fmt.Fprintf(w, `{
			"tag_name":"v9.9.9",
			"body":"silent notes",
			"assets":[
				{"name":"Agent-notify-Setup-v9.9.9.exe","url":"%s/setup"},
				{"name":"SHA256SUMS.txt","url":"%s/checksums"}
			]
		}`, base, base)
	}))
	defer server.Close()

	original := newUpdateClient
	newUpdateClient = func() *update.Client {
		return &update.Client{HTTPClient: server.Client(), Repository: "owner/repo", APIBaseURL: server.URL}
	}
	t.Cleanup(func() { newUpdateClient = original })

	app := &WidgetApp{}
	app.checkUpdateSilently()

	if release, ok := app.pendingUpdate(); !ok || release.Version != "9.9.9" {
		t.Fatalf("silent check pending update = %+v ok=%v", release, ok)
	}
	if label, primary := app.updateButtonState(); label != "升级 v9.9.9" || !primary {
		t.Fatalf("button after silent check = %q primary=%v", label, primary)
	}
	if app.updateState.background {
		t.Fatal("background flag should be cleared after silent check")
	}
}
