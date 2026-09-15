//go:build windows

package ui

import (
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
