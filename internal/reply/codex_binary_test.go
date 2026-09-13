package reply

import (
	"context"
	"os"
	"path/filepath"
	"testing"
	"time"
)

func TestCheckCodexQueueUsesDiscoveredCLI(t *testing.T) {
	localAppData := t.TempDir()
	installRoot := filepath.Join(localAppData, "OpenAI", "Codex", "bin")
	want := writeCodexBinary(t, filepath.Join(installRoot, "cli-current"), time.Unix(200, 0))

	t.Setenv("LOCALAPPDATA", localAppData)
	t.Setenv(codexBinaryEnv, "")
	t.Setenv("PATH", t.TempDir())

	var gotBinary string
	got, err := checkCodexQueue(
		context.Background(),
		"",
		processRunnerFunc(func(_ context.Context, binary string, _ ...string) ([]byte, error) {
			gotBinary = binary
			return []byte("Usage: codex queue"), nil
		}),
	)
	if err != nil {
		t.Fatalf("checkCodexQueue: %v", err)
	}
	if got != want || gotBinary != want {
		t.Fatalf("checkCodexQueue binary = %q and runner saw %q, want %q", got, gotBinary, want)
	}
}

func TestResolveCodexBinaryPrefersExplicitValue(t *testing.T) {
	t.Setenv(codexBinaryEnv, "env-codex")
	got, err := resolveCodexBinary("  explicit-codex  ")
	if err != nil {
		t.Fatalf("resolveCodexBinary: %v", err)
	}
	if got != "explicit-codex" {
		t.Fatalf("resolveCodexBinary = %q, want explicit-codex", got)
	}
}

func TestResolveCodexBinaryUsesEnvironmentBeforePath(t *testing.T) {
	t.Setenv(codexBinaryEnv, "env-codex")
	got, err := resolveCodexBinary("")
	if err != nil {
		t.Fatalf("resolveCodexBinary: %v", err)
	}
	if got != "env-codex" {
		t.Fatalf("resolveCodexBinary = %q, want env-codex", got)
	}
}

func TestResolveCodexBinaryFallsBackToInstalledCLI(t *testing.T) {
	localAppData := t.TempDir()
	installRoot := filepath.Join(localAppData, "OpenAI", "Codex", "bin")
	older := writeCodexBinary(t, filepath.Join(installRoot, "older"), time.Unix(100, 0))
	newer := writeCodexBinary(t, filepath.Join(installRoot, "newer"), time.Unix(200, 0))

	t.Setenv("LOCALAPPDATA", localAppData)
	t.Setenv(codexBinaryEnv, "")
	t.Setenv("PATH", t.TempDir())

	got, err := resolveCodexBinary("")
	if err != nil {
		t.Fatalf("resolveCodexBinary: %v", err)
	}
	if got != newer {
		t.Fatalf("resolveCodexBinary = %q, want newest %q (older %q)", got, newer, older)
	}
}

func writeCodexBinary(t *testing.T, directory string, modified time.Time) string {
	t.Helper()
	if err := os.MkdirAll(directory, 0700); err != nil {
		t.Fatalf("MkdirAll: %v", err)
	}
	path := filepath.Join(directory, codexBinaryName)
	if err := os.WriteFile(path, []byte("test"), 0600); err != nil {
		t.Fatalf("WriteFile: %v", err)
	}
	if err := os.Chtimes(path, modified, modified); err != nil {
		t.Fatalf("Chtimes: %v", err)
	}
	return path
}
