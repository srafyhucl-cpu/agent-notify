//go:build !windows

package sysproc

import "os/exec"

// ConfigureHidden 在非 Windows 平台为空实现。
func ConfigureHidden(command *exec.Cmd) {}
