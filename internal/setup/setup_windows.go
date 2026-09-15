//go:build windows

package setup

import (
	"context"
	"os/exec"
	"syscall"
)

const createNoWindow = 0x08000000

func runHidden(ctx context.Context, name string, args ...string) ([]byte, error) {
	command := exec.CommandContext(ctx, name, args...)
	command.SysProcAttr = &syscall.SysProcAttr{
		HideWindow:    true,
		CreationFlags: createNoWindow,
	}
	return command.CombinedOutput()
}
