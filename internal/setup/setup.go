package setup

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"strings"
	"time"
)

type Options struct {
	Version    string
	InstallDir string
	ScriptPath string
	StateFile  string
	LogFile    string
	Force      bool
}

type commandRunner func(context.Context, string, ...string) ([]byte, error)

type state struct {
	Version     string `json:"version"`
	CompletedAt string `json:"completedAt"`
}

func Ensure(ctx context.Context, options Options) error {
	return ensure(ctx, options, runHidden)
}

func ensure(ctx context.Context, options Options, runner commandRunner) error {
	if !options.Force && IsComplete(options.StateFile, options.Version) {
		return nil
	}
	if strings.TrimSpace(options.InstallDir) == "" {
		return errors.New("安装目录为空")
	}
	if _, err := os.Stat(options.ScriptPath); err != nil {
		if os.IsNotExist(err) {
			// 旧版 install.ps1/ZIP 安装只落盘了 exe，这里给用户可以照着做的提示。
			return fmt.Errorf("安装目录缺少 install.ps1（%s）：请重新运行安装器或 install.ps1", options.ScriptPath)
		}
		return fmt.Errorf("无法读取首次配置程序 %s：%w", options.ScriptPath, err)
	}
	args := []string{
		"-NoProfile", "-ExecutionPolicy", "Bypass", "-File", options.ScriptPath,
		"-ConfigureOnly", "-InstallDir", options.InstallDir,
		"-SkipWidgetLaunch", "-SkipLoginLaunch", "-SkipShortcuts",
	}
	output, err := runner(ctx, "powershell.exe", args...)
	if err != nil {
		_ = os.MkdirAll(filepath.Dir(options.LogFile), 0700)
		_ = os.WriteFile(options.LogFile, output, 0600)
		return fmt.Errorf("首次接入失败：%w", err)
	}
	if err := writeState(options.StateFile, options.Version); err != nil {
		return err
	}
	// 接入成功后清掉上一次的失败日志，避免用户按 README 查看时看到过期报错。
	if err := os.Remove(options.LogFile); err != nil && !os.IsNotExist(err) {
		// 日志清理失败不影响接入结果
	}
	return nil
}

func IsComplete(stateFile, version string) bool {
	data, err := os.ReadFile(stateFile)
	if err != nil {
		return false
	}
	var value state
	return json.Unmarshal(data, &value) == nil && value.Version == version
}

func writeState(path, version string) error {
	if err := os.MkdirAll(filepath.Dir(path), 0700); err != nil {
		return err
	}
	data, err := json.MarshalIndent(state{
		Version:     version,
		CompletedAt: time.Now().Format(time.RFC3339),
	}, "", "  ")
	if err != nil {
		return err
	}
	temporary := path + ".new"
	if err := os.WriteFile(temporary, append(data, '\n'), 0600); err != nil {
		return err
	}
	return os.Rename(temporary, path)
}
