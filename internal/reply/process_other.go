//go:build !windows

package reply

import "os/exec"

func configureHiddenProcess(_ *exec.Cmd) {}
