//go:build windows

package reply

import (
	"os/exec"
	"testing"
)

func TestConfigureHiddenProcess(t *testing.T) {
	command := exec.Command("codex", "queue", "--help")
	configureHiddenProcess(command)
	if command.SysProcAttr == nil || !command.SysProcAttr.HideWindow {
		t.Fatal("hidden process was not configured")
	}
	if command.SysProcAttr.CreationFlags&createNoWindow == 0 {
		t.Fatal("CREATE_NO_WINDOW was not configured")
	}
}
