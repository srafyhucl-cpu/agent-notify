package reply

import (
	"context"
	"os"
	"strings"
	"testing"
	"time"
)

const (
	integrationCodexThreadEnv = "AGENT_NOTIFY_CODEX_INTEGRATION_THREAD"
	integrationCodexBinaryEnv = "AGENT_NOTIFY_CODEX_INTEGRATION_BIN"
	integrationCodexTimeout   = 30 * time.Second
	integrationProbeMessage   = "agent-notify codex queue integration probe"
)

func TestCodexQueueRunnerRealCLIIntegration(t *testing.T) {
	threadID := strings.TrimSpace(os.Getenv(integrationCodexThreadEnv))
	if threadID == "" {
		t.Skip("set " + integrationCodexThreadEnv + " to run against an isolated Codex checkout")
	}
	binary := strings.TrimSpace(os.Getenv(integrationCodexBinaryEnv))
	if binary == "" {
		binary = "codex"
	}

	runner := CodexQueueRunner{
		Binary:  binary,
		Timeout: integrationCodexTimeout,
	}
	if err := runner.Queue(context.Background(), threadID, integrationProbeMessage); err != nil {
		t.Fatalf("Queue real Codex CLI: %v", err)
	}
}
