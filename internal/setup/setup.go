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
		return fmt.Errorf("首次配置程序不存在：%w", err)
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
	return writeState(options.StateFile, options.Version)
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
