//go:build windows

package reply

import (
	"os"
	"strings"
	"testing"
)

func TestProcessImageNameFindsCurrentProcess(t *testing.T) {
	name, ok := processImageName(os.Getpid())
	if !ok {
		t.Fatalf("processImageName(%d) reported a missing process", os.Getpid())
	}
	if strings.TrimSpace(name) == "" {
		t.Fatal("processImageName() returned an empty image name for the test process")
	}
	if !strings.HasSuffix(strings.ToLower(name), ".exe") {
		t.Fatalf("processImageName() = %q, want an .exe path", name)
	}
}

func TestProcessImageNameRejectsInvalidPID(t *testing.T) {
	for _, pid := range []int{0, -1} {
		if _, ok := processImageName(pid); ok {
			t.Fatalf("processImageName(%d) reported an existing process", pid)
		}
	}
}
