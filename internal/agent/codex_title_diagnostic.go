package agent

import (
	"encoding/json"
	"os"
	"strings"
	"time"

	"github.com/srafyhucl-cpu/agent-notify/internal/config"
)

type codexTitleDiagnostic struct {
	Timestamp      string `json:"timestamp"`
	ThreadID       string `json:"threadID"`
	Source         string `json:"source"`
	FailureStage   string `json:"failureStage,omitempty"`
	SQLiteCode     int    `json:"sqliteCode,omitempty"`
	Retries        int    `json:"retries,omitempty"`
	FallbackSource string `json:"fallbackSource,omitempty"`
	Warning        string `json:"warning,omitempty"`
	Detail         string `json:"detail,omitempty"`
}

func writeCodexTitleDiagnostic(threadID string, resolution codexTitleResolution) {
	threadID = strings.TrimSpace(threadID)
	if threadID == "" {
		return
	}
	entry := codexTitleDiagnostic{
		Timestamp:      time.Now().Format(time.RFC3339Nano),
		ThreadID:       threadID,
		Source:         resolution.Source,
		FailureStage:   resolution.FailureStage,
		SQLiteCode:     resolution.SQLiteCode,
		Retries:        resolution.Retries,
		FallbackSource: resolution.FallbackSource,
		Warning:        resolution.Warning,
		Detail:         resolution.FailureDetail,
	}
	data, err := json.Marshal(entry)
	if err != nil {
		return
	}

	paths := config.GetPaths()
	if err := os.MkdirAll(paths.TempDir, 0700); err != nil {
		return
	}
	file, err := os.OpenFile(paths.CodexTitleLog, os.O_APPEND|os.O_CREATE|os.O_WRONLY, 0600)
	if err != nil {
		return
	}
	defer file.Close()
	_, _ = file.Write(append(data, '\n'))
}
