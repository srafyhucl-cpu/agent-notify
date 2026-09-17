package reply

import (
	"fmt"
	"time"

	"github.com/srafyhucl-cpu/agent-notify/internal/config"
	"github.com/srafyhucl-cpu/agent-notify/internal/diag"
)

func writeReplyDiagnostic(format string, args ...any) {
	writeReplyDiagnosticAt(time.Now(), format, args...)
}

func writeReplyDiagnosticAt(now time.Time, format string, args ...any) {
	paths := config.GetPaths()
	line := fmt.Sprintf("%s %s\n", now.Format(time.RFC3339), fmt.Sprintf(format, args...))
	diag.Append(paths.ReplyDebugLog, line)
}
