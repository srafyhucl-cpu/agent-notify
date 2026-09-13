package main

import (
	"encoding/json"
	"io"
	"os"
	"testing"

	"github.com/srafyhucl-cpu/agent-notify/internal/reply"
)

func TestReplyCheckFailureEmitsJSON(t *testing.T) {
	reader, writer, err := os.Pipe()
	if err != nil {
		t.Fatalf("Pipe: %v", err)
	}
	defer reader.Close()

	originalStdout := os.Stdout
	os.Stdout = writer
	code := replyCheckFailure(replyCheckOutput{Path: "diagnostics.log"}, "resolver unavailable", true)
	_ = writer.Close()
	os.Stdout = originalStdout

	if code != replyCheckExitFailed {
		t.Fatalf("exit code = %d, want %d", code, replyCheckExitFailed)
	}
	data, err := io.ReadAll(reader)
	if err != nil {
		t.Fatalf("ReadAll: %v", err)
	}
	var output replyCheckOutput
	if err := json.Unmarshal(data, &output); err != nil {
		t.Fatalf("Unmarshal(%q): %v", data, err)
	}
	if output.Gate.Status != reply.ReplyGateFailed {
		t.Fatalf("gate status = %q", output.Gate.Status)
	}
	if output.Error != "resolver unavailable" {
		t.Fatalf("error = %q", output.Error)
	}
}
