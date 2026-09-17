package main

import (
	"io"
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/srafyhucl-cpu/agent-notify/internal/clawbot"
	"github.com/srafyhucl-cpu/agent-notify/internal/notify"
)

// captureStdout 临时把 os.Stdout 指向管道，收集 fn 打印的内容后还原。
func captureStdout(t *testing.T, fn func()) string {
	t.Helper()
	reader, writer, err := os.Pipe()
	if err != nil {
		t.Fatalf("Pipe: %v", err)
	}
	original := os.Stdout
	os.Stdout = writer
	defer func() { os.Stdout = original }()

	fn()

	if err := writer.Close(); err != nil {
		t.Fatalf("Close: %v", err)
	}
	data, err := io.ReadAll(reader)
	if err != nil {
		t.Fatalf("ReadAll: %v", err)
	}
	_ = reader.Close()
	return string(data)
}

func TestStdinAvailable(t *testing.T) {
	original := os.Stdin
	defer func() { os.Stdin = original }()

	tempFile, err := os.CreateTemp(t.TempDir(), "stdin-*")
	if err != nil {
		t.Fatal(err)
	}
	defer tempFile.Close()

	reader, writer, err := os.Pipe()
	if err != nil {
		t.Fatal(err)
	}
	defer reader.Close()
	defer writer.Close()

	devNull, err := os.Open(os.DevNull)
	if err != nil {
		t.Fatalf("open %s: %v", os.DevNull, err)
	}
	defer devNull.Close()

	tests := []struct {
		name  string
		stdin *os.File
		want  bool
	}{
		{"管道视为可用（无参数时读取 stdin 通知）", reader, true},
		{"普通文件视为可用", tempFile, true},
		{"字符设备（NUL）视为不可用", devNull, false},
	}
	for _, test := range tests {
		t.Run(test.name, func(t *testing.T) {
			os.Stdin = test.stdin
			if got := stdinAvailable(); got != test.want {
				t.Fatalf("stdinAvailable() = %v, want %v", got, test.want)
			}
		})
	}
}

func TestLoginStatusText(t *testing.T) {
	tests := []struct {
		name   string
		status string
		want   string
	}{
		{"等待扫码", clawbot.StatusWait, "等待扫码"},
		{"已扫码待确认", clawbot.StatusScanned, "已扫码，请在微信中确认"},
		{"切换扫码节点", clawbot.StatusScannedRedirect, "正在切换扫码节点"},
		{"需要配对码", clawbot.StatusNeedVerifyCode, "需要在微信中查看数字配对码"},
		{"已确认", clawbot.StatusConfirmed, "已确认"},
		{"二维码过期", clawbot.StatusExpired, "二维码已过期，正在重新获取"},
		{"配对码错误过多", clawbot.StatusVerifyBlocked, "配对码错误次数过多，正在重新获取二维码"},
		{"空状态静默", "", ""},
		{"未知状态静默", "unknown-status", ""},
	}
	for _, test := range tests {
		t.Run(test.name, func(t *testing.T) {
			if got := loginStatusText(test.status); got != test.want {
				t.Fatalf("loginStatusText(%q) = %q, want %q", test.status, got, test.want)
			}
		})
	}
}

func TestLoginStatus(t *testing.T) {
	tests := []struct {
		name   string
		status clawbot.Status
		want   string
	}{
		{"未登录", clawbot.Status{}, "未登录"},
		{"未登录优先于失效", clawbot.Status{Stale: true}, "未登录"},
		{"登录失效", clawbot.Status{LoggedIn: true, Stale: true}, "登录已失效，请重新扫码"},
		{"已登录未建会话", clawbot.Status{LoggedIn: true}, "已登录 · 等待微信消息建立会话"},
		{"会话就绪", clawbot.Status{LoggedIn: true, SessionReady: true}, "已登录 · 会话就绪"},
		{"会话就绪带提示", clawbot.Status{LoggedIn: true, SessionReady: true, UserHint: "机器人: bot-1"}, "已登录 · 会话就绪 · 机器人: bot-1"},
	}
	for _, test := range tests {
		t.Run(test.name, func(t *testing.T) {
			if got := loginStatus(test.status); got != test.want {
				t.Fatalf("loginStatus(%+v) = %q, want %q", test.status, got, test.want)
			}
		})
	}
}

func TestSessionStatus(t *testing.T) {
	tests := []struct {
		name   string
		status clawbot.Status
		want   string
	}{
		{"未登录", clawbot.Status{}, "未建立（未登录）"},
		{"未登录优先于失效", clawbot.Status{Stale: true}, "未建立（未登录）"},
		{"登录失效", clawbot.Status{LoggedIn: true, Stale: true}, "未建立（登录已失效）"},
		{"已登录未建会话", clawbot.Status{LoggedIn: true}, "未建立（请先给 ClawBot 发一条微信消息）"},
		{"会话就绪", clawbot.Status{LoggedIn: true, SessionReady: true}, "就绪"},
	}
	for _, test := range tests {
		t.Run(test.name, func(t *testing.T) {
			if got := sessionStatus(test.status); got != test.want {
				t.Fatalf("sessionStatus(%+v) = %q, want %q", test.status, got, test.want)
			}
		})
	}
}

func TestOnOff(t *testing.T) {
	tests := []struct {
		name    string
		enabled bool
		want    string
	}{
		{"开启", true, "开启"},
		{"暂停", false, "暂停"},
	}
	for _, test := range tests {
		t.Run(test.name, func(t *testing.T) {
			if got := onOff(test.enabled); got != test.want {
				t.Fatalf("onOff(%v) = %q, want %q", test.enabled, got, test.want)
			}
		})
	}
}

func TestEmptyAs(t *testing.T) {
	tests := []struct {
		name     string
		value    string
		fallback string
		want     string
	}{
		{"空串回落", "", "关闭", "关闭"},
		{"纯空白回落", "   \t\n ", "关闭", "关闭"},
		{"非空原样返回", "22:00-08:00", "关闭", "22:00-08:00"},
		{"仅裁剪用于判空", " 22:00 ", "关闭", " 22:00 "},
	}
	for _, test := range tests {
		t.Run(test.name, func(t *testing.T) {
			if got := emptyAs(test.value, test.fallback); got != test.want {
				t.Fatalf("emptyAs(%q, %q) = %q, want %q", test.value, test.fallback, got, test.want)
			}
		})
	}
}

func TestInstalledStatus(t *testing.T) {
	tests := []struct {
		name      string
		installed bool
		want      string
	}{
		{"已安装", true, "已安装"},
		{"未安装", false, "未安装"},
	}
	for _, test := range tests {
		t.Run(test.name, func(t *testing.T) {
			if got := installedStatus(test.installed); got != test.want {
				t.Fatalf("installedStatus(%v) = %q, want %q", test.installed, got, test.want)
			}
		})
	}
}

func TestFileExists(t *testing.T) {
	tempDir := t.TempDir()
	existing := filepath.Join(tempDir, "exists.txt")
	if err := os.WriteFile(existing, []byte("ok"), 0600); err != nil {
		t.Fatal(err)
	}
	missing := filepath.Join(tempDir, "missing.txt")

	tests := []struct {
		name string
		path string
		want bool
	}{
		{"文件存在", existing, true},
		{"目录也算存在", tempDir, true},
		{"文件不存在", missing, false},
		{"空路径不存在", "", false},
	}
	for _, test := range tests {
		t.Run(test.name, func(t *testing.T) {
			if got := fileExists(test.path); got != test.want {
				t.Fatalf("fileExists(%q) = %v, want %v", test.path, got, test.want)
			}
		})
	}
}

func TestFirstHistory(t *testing.T) {
	if got := firstHistory(nil); got != nil {
		t.Fatalf("firstHistory(nil) = %v, want nil", got)
	}
	if got := firstHistory([]notify.HistoryItem{}); got != nil {
		t.Fatalf("firstHistory(empty) = %v, want nil", got)
	}

	items := []notify.HistoryItem{
		{Title: "第一条", Agent: "opencode", Status: notify.StatusSuccess},
		{Title: "第二条", Agent: "codex", Status: notify.StatusFailed},
	}
	got, ok := firstHistory(items).(notify.HistoryItem)
	if !ok {
		t.Fatalf("firstHistory 返回类型 = %T, want notify.HistoryItem", firstHistory(items))
	}
	if got.Title != "第一条" {
		t.Fatalf("firstHistory 首条 = %+v, want 第一条", got)
	}
}

// TestPrintHelpMentionsReplyCheck 防回归：帮助文本必须继续暴露 reply-check 命令。
func TestPrintHelpMentionsReplyCheck(t *testing.T) {
	output := captureStdout(t, printHelp)
	if !strings.Contains(output, "reply-check") {
		t.Fatalf("帮助文本缺少 reply-check 命令:\n%s", output)
	}
}
