//go:build !windows

package reply

import "context"

// agentProcess 在非 Windows 平台不参与语言服务发现。
type agentProcess struct {
	PID         uint32
	Executable  string
	CommandLine string
}

func runningAgentExecutablePaths(_ ...string) []string {
	return nil
}

func runningAgentProcesses(_ context.Context, _ ...string) []agentProcess {
	return nil
}

func listeningTCPPorts(_ uint32) []int {
	return nil
}
