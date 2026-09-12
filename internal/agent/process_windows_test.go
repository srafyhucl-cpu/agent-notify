//go:build windows

package agent

import (
	"os/exec"
	"syscall"
	"testing"
)

func TestConfigureHiddenProcessSuppressesConsoleWindow(t *testing.T) {
	command := exec.Command("cmd.exe")
	configureHiddenProcess(command)

	if command.SysProcAttr == nil {
		t.Fatal("SysProcAttr must be set or Windows creates a visible console for the child process")
	}
	if !command.SysProcAttr.HideWindow {
		t.Fatal("HideWindow must be enabled to prevent the console flash")
	}
	if command.SysProcAttr.CreationFlags&createNoWindow == 0 {
		t.Fatalf("CREATE_NO_WINDOW must be set, got CreationFlags=0x%X", command.SysProcAttr.CreationFlags)
	}
	if command.SysProcAttr.CreationFlags&syscall.CREATE_NEW_PROCESS_GROUP != 0 {
		t.Fatal("unexpected process group flag")
	}
}
