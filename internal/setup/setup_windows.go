//go:build windows

package setup

import (
	"context"
	"os/exec"

	"github.com/srafyhucl-cpu/agent-notify/internal/sysproc"
)

func runHidden(ctx context.Context, name string, args ...string) ([]byte, error) {
	command := exec.CommandContext(ctx, name, args...)
	sysproc.ConfigureHidden(command)
	return command.CombinedOutput()
}
