//go:build windows

package ui

import (
	"os"
	"path/filepath"
	"testing"

	"github.com/srafyhucl-cpu/agent-notify/internal/config"
)

// 配置损坏时不能把文件覆盖成默认值，并且必须报告失败。
func TestMutateConfigDoesNotOverwriteBrokenConfig(t *testing.T) {
	dir := t.TempDir()
	t.Setenv("AGENT_NOTIFY_CONFIG_DIR", dir)
	t.Setenv("AGENT_NOTIFY_TEMP_DIR", filepath.Join(dir, "temp"))

	configPath := filepath.Join(dir, "config.json")
	broken := `{"quietHours": "23-8",`
	if err := os.WriteFile(configPath, []byte(broken), 0600); err != nil {
		t.Fatal(err)
	}

	app := WidgetApp{}
	applied := false
	if app.mutateConfig(func(cfg *config.AppConfig) { applied = true }) {
		t.Fatal("配置损坏时 mutateConfig 不应报告成功")
	}
	if applied {
		t.Fatal("读取失败时不应执行修改回调")
	}
	after, err := os.ReadFile(configPath)
	if err != nil {
		t.Fatal(err)
	}
	if string(after) != broken {
		t.Fatalf("损坏的配置被覆盖：%q", after)
	}
}

func TestMutateConfigPersistsChanges(t *testing.T) {
	dir := t.TempDir()
	t.Setenv("AGENT_NOTIFY_CONFIG_DIR", dir)
	t.Setenv("AGENT_NOTIFY_TEMP_DIR", filepath.Join(dir, "temp"))

	app := WidgetApp{}
	if !app.mutateConfig(func(cfg *config.AppConfig) { cfg.Theme = "light" }) {
		t.Fatal("mutateConfig 应成功")
	}
	loaded, err := config.LoadConfig("")
	if err != nil {
		t.Fatalf("LoadConfig: %v", err)
	}
	if loaded.Theme != "light" {
		t.Fatalf("theme = %q, want light", loaded.Theme)
	}
}

// 字体按 DPI 缓存：重复取用不重复创建，DPI 变化后才重建。
func TestUIFontsCacheByDPI(t *testing.T) {
	previous := uiDPI
	defer setUIDPI(previous)

	app := WidgetApp{}
	setUIDPI(96)
	first := app.uiFonts()
	if first.title == 0 || first.base == 0 || first.strong == 0 || first.small == 0 || first.icon == 0 {
		t.Fatalf("字体句柄不完整：%+v", first)
	}
	again := app.uiFonts()
	if again.title != first.title || again.icon != first.icon {
		t.Fatal("相同 DPI 下应复用缓存字体")
	}

	setUIDPI(144)
	scaled := app.uiFonts()
	if scaled.title == first.title {
		t.Fatal("DPI 变化后应重建字体")
	}
	defer app.releaseFonts()
}
