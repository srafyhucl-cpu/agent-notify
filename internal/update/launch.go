//go:build windows

package update

import (
	"errors"
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"strings"
)

const powershellUTF8BOM = "\xEF\xBB\xBF"

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
	if prepared.Kind == ArtifactInstaller {
		artifactPath := strings.TrimSpace(prepared.ArtifactPath)
		if artifactPath == "" {
			artifactPath = installerPath
		}
		if _, err := os.Stat(artifactPath); err != nil {
			return fmt.Errorf("更新安装器不可用：%w", err)
		}
		command := installerCommand(artifactPath, installerArgs)
		if err := command.Start(); err != nil {
			return fmt.Errorf("启动更新安装器失败：%w", err)
		}
		return nil
	}

	logPath = strings.TrimSpace(logPath)
	if logPath == "" {
		return errors.New("更新日志路径为空")
	}
	if err := os.MkdirAll(filepath.Dir(logPath), 0700); err != nil {
		return fmt.Errorf("创建更新日志目录失败：%w", err)
	}

	wrapperPath := filepath.Join(filepath.Dir(installerPath), "apply-update.ps1")
	script := buildUpdaterScript(installerPath, logPath, installerArgs)
	if err := os.WriteFile(wrapperPath, []byte(powershellUTF8BOM+script), 0700); err != nil {
		return fmt.Errorf("写入更新启动器失败：%w", err)
	}

	command := exec.Command("powershell.exe", "-NoProfile", "-ExecutionPolicy", "Bypass", "-File", wrapperPath)
	command.Dir = filepath.Dir(installerPath)
	configureHiddenProcess(command)
	if err := command.Start(); err != nil {
		return fmt.Errorf("启动更新安装器失败：%w", err)
	}
	return nil
}

func installerCommand(installerPath string, installerArgs []string) *exec.Cmd {
	args := make([]string, 0, len(installerArgs)+1)
	args = append(args, "/NORESTART")
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
