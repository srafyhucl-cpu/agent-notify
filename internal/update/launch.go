//go:build windows

package update

import (
	"errors"
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"strings"
	"time"
)

const (
	powershellUTF8BOM = "\xEF\xBB\xBF"
	// installerEarlyExitWindow 是判定"安装器立即失败"的观察窗口：正常静默安装会持续数秒以上，
	// 在窗口内退出且退出码非零说明参数、权限或安装包有问题，必须立刻告诉用户而不是直接销毁悬浮窗。
	installerEarlyExitWindow = 2 * time.Second
)

// Launch starts a native installer visibly, or a legacy archive installer in a
// detached hidden process. installerArgs are appended verbatim.
func (prepared PreparedUpdate) Launch(logPath string, installerArgs ...string) error {
	installerPath := strings.TrimSpace(prepared.InstallerPath)
	if installerPath == "" {
		return errors.New("更新安装器路径为空")
	}
	if _, err := os.Stat(installerPath); err != nil {
		return fmt.Errorf("更新安装器不可用：%w", err)
	}
	logPath = strings.TrimSpace(logPath)
	if logPath == "" {
		return errors.New("更新日志路径为空")
	}
	if err := os.MkdirAll(filepath.Dir(logPath), 0700); err != nil {
		return fmt.Errorf("创建更新日志目录失败：%w", err)
	}

	if prepared.Kind == ArtifactInstaller {
		artifactPath := strings.TrimSpace(prepared.ArtifactPath)
		if artifactPath == "" {
			artifactPath = installerPath
		}
		if _, err := os.Stat(artifactPath); err != nil {
			return fmt.Errorf("更新安装器不可用：%w", err)
		}
		command := installerCommand(artifactPath, logPath, installerArgs)
		if err := startAndWatch(command, installerEarlyExitWindow); err != nil {
			return fmt.Errorf("更新安装器启动失败：%w（安装日志：%s）", err, logPath)
		}
		return nil
	}

	wrapperPath := filepath.Join(filepath.Dir(installerPath), "apply-update.ps1")
	script := buildUpdaterScript(installerPath, logPath, installerArgs)
	if err := os.WriteFile(wrapperPath, []byte(powershellUTF8BOM+script), 0700); err != nil {
		return fmt.Errorf("写入更新启动器失败：%w", err)
	}

	command := exec.Command("powershell.exe", "-NoProfile", "-ExecutionPolicy", "Bypass", "-File", wrapperPath)
	command.Dir = filepath.Dir(installerPath)
	configureHiddenProcess(command)
	if err := startAndWatch(command, installerEarlyExitWindow); err != nil {
		return fmt.Errorf("更新安装器启动失败：%w（安装日志：%s）", err, logPath)
	}
	return nil
}

// startAndWatch 启动进程，并在 window 内观察是否立即失败。
func startAndWatch(command *exec.Cmd, window time.Duration) error {
	if err := command.Start(); err != nil {
		return err
	}
	done := make(chan error, 1)
	go func() { done <- command.Wait() }()
	timer := time.NewTimer(window)
	defer timer.Stop()
	select {
	case err := <-done:
		// 立即退出：非零即失败；零退出说明安装器瞬间完成，同样视为成功。
		return err
	case <-timer.C:
		return nil
	}
}

func installerCommand(installerPath, logPath string, installerArgs []string) *exec.Cmd {
	args := make([]string, 0, len(installerArgs)+3)
	args = append(args, "/SILENT", "/NORESTART")
	if trimmed := strings.TrimSpace(logPath); trimmed != "" {
		args = append(args, "/LOG="+trimmed)
	}
	args = append(args, installerArgs...)
	command := exec.Command(installerPath, args...)
	command.Dir = filepath.Dir(installerPath)
	return command
}

func buildUpdaterScript(installerPath, logPath string, installerArgs []string) string {
	quotedInstaller := powerShellSingleQuoted(installerPath)
	quotedLog := powerShellSingleQuoted(logPath)
	quotedArgs := make([]string, 0, len(installerArgs))
	for _, argument := range installerArgs {
		quotedArgs = append(quotedArgs, powerShellSingleQuoted(argument))
	}
	invocation := "& powershell.exe -NoProfile -ExecutionPolicy Bypass -File " + quotedInstaller
	if len(quotedArgs) > 0 {
		invocation += " " + strings.Join(quotedArgs, " ")
	}
	return fmt.Sprintf(`$ErrorActionPreference = 'Stop'
$logPath = %s
try {
  $output = %s 2>&1
  $output | Set-Content -LiteralPath $logPath -Encoding UTF8
  if ($LASTEXITCODE -ne 0) {
    throw "安装器退出码 $LASTEXITCODE"
  }
} catch {
  $_ | Out-String | Add-Content -LiteralPath $logPath -Encoding UTF8
  Add-Type -AssemblyName PresentationFramework
  $message = "Agent-notify 自动更新失败。" + [Environment]::NewLine + [Environment]::NewLine + "详情见：" + [Environment]::NewLine + $logPath
  [System.Windows.MessageBox]::Show(
    $message,
    "Agent-notify 更新失败",
    [System.Windows.MessageBoxButton]::OK,
    [System.Windows.MessageBoxImage]::Error
  ) | Out-Null
  exit 1
}
`, quotedLog, invocation)
}

func powerShellSingleQuoted(value string) string {
	return "'" + strings.ReplaceAll(value, "'", "''") + "'"
}
