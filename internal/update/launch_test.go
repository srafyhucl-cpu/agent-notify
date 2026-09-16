//go:build windows

package update

import (
	"os/exec"
	"strings"
	"testing"
	"time"
)

// 安装器必须带 /LOG，失败时用户才能拿到日志路径。
func TestInstallerCommandIncludesLog(t *testing.T) {
	command := installerCommand(`C:\tmp\setup.exe`, `C:\tmp\updates\last-update.log`, []string{"/DIR=C:\\app"})
	joined := strings.Join(command.Args, " ")
	for _, want := range []string{"/SILENT", "/NORESTART", `/LOG=C:\tmp\updates\last-update.log`, `/DIR=C:\app`} {
		if !strings.Contains(joined, want) {
			t.Fatalf("命令缺少 %q：%s", want, joined)
		}
	}
}

// 立即非零退出必须被识别为失败，避免界面在安装失败时直接退出。
func TestStartAndWatchReportsImmediateFailure(t *testing.T) {
	command := exec.Command("cmd.exe", "/c", "exit 3")
	if err := startAndWatch(command, 5*time.Second); err == nil {
		t.Fatal("退出码非零时应返回错误")
	}
}

func TestStartAndWatchTreatsRunningProcessAsStarted(t *testing.T) {
	// ping 约 2 秒，100ms 观察窗口内仍在运行，应视为启动成功。
	command := exec.Command("cmd.exe", "/c", "ping -n 3 127.0.0.1 > nul")
	start := time.Now()
	if err := startAndWatch(command, 100*time.Millisecond); err != nil {
		t.Fatalf("仍在运行的安装器不应报错：%v", err)
	}
	if elapsed := time.Since(start); elapsed > time.Second {
		t.Fatalf("观察窗口未生效，耗时 %v", elapsed)
	}
}
