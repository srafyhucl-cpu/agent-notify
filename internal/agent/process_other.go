//go:build !windows

package agent

import "os/exec"

func configureHiddenProcess(_ *exec.Cmd) {}
