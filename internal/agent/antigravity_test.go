package agent

import (
	"os"
	"path/filepath"
	"testing"
)

func TestAntigravityTranscriptParsing(t *testing.T) {
	tempDir := t.TempDir()
	transcriptFile := filepath.Join(tempDir, "transcript.jsonl")

	lines := []string{
		`{"role":"user","content":"<USER_REQUEST>**Task**: 实施 Go 语言单文件 exe 升级</USER_REQUEST>"}`,
		`{"role":"assistant","content":"第一步设计架构"}`,
		`{"role":"user","content":"继续"}`,
		`{"role":"assistant","content":"第二步编写测试并成功验证。"}`,
	}

	content := ""
	for _, l := range lines {
		content += l + "\n"
	}
	if err := os.WriteFile(transcriptFile, []byte(content), 0644); err != nil {
		t.Fatalf("failed to write transcript: %v", err)
	}

	// Test dry-run invocation
	payload := `{"fullyIdle":true,"transcriptPath":"` + filepath.ToSlash(transcriptFile) + `"}`

	// Setting ANTIGRAVITY_NOTIFY_DRYRUN ensures no real network push is made
	t.Setenv("ANTIGRAVITY_NOTIFY_DRYRUN", "1")
	HandleAntigravity(payload, true)
}

func TestAntigravityTranscriptLargeLine(t *testing.T) {
	tempDir := t.TempDir()
	transcriptFile := filepath.Join(tempDir, "transcript_large.jsonl")

	// Generate a line exceeding bufio.MaxScanTokenSize (64KB)
	largeText := make([]byte, 100*1024)
	for i := range largeText {
		largeText[i] = 'A'
	}

	lines := []string{
		`{"role":"user","content":"<USER_REQUEST>**Task**: 大文件测试</USER_REQUEST>"}`,
		`{"role":"assistant","content":"` + string(largeText) + `"}`,
		`{"role":"assistant","content":"最后结果：成功处理超大行。"}`,
	}

	content := ""
	for _, l := range lines {
		content += l + "\n"
	}
	if err := os.WriteFile(transcriptFile, []byte(content), 0644); err != nil {
		t.Fatalf("failed to write transcript: %v", err)
	}

	payload := `{"fullyIdle":true,"transcriptPath":"` + filepath.ToSlash(transcriptFile) + `"}`
	t.Setenv("ANTIGRAVITY_NOTIFY_DRYRUN", "1")
	HandleAntigravity(payload, true)
}
