package reply

import (
	"fmt"
	"os"
	"time"

	"github.com/srafyhucl-cpu/agent-notify/internal/config"
)

func writeReplyDiagnostic(format string, args ...any) {
	writeReplyDiagnosticAt(time.Now(), format, args...)
}

func writeReplyDiagnosticAt(now time.Time, format string, args ...any) {
	paths := config.GetPaths()
	if err := os.MkdirAll(paths.TempDir, privateDirPerm); err != nil {
		return
	}
	file, err := os.OpenFile(paths.ReplyDebugLog, os.O_APPEND|os.O_CREATE|os.O_WRONLY, privateFilePerm)
	if err != nil {
		return
	}
	defer file.Close()
	_, _ = fmt.Fprintf(file, "%s %s\n", now.Format(time.RFC3339), fmt.Sprintf(format, args...))
}
