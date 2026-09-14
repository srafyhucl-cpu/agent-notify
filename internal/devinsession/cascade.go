package devinsession

import (
	"encoding/json"
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"strings"

	"github.com/srafyhucl-cpu/agent-notify/internal/winsqlite"
)

const (
	desktopDatabaseEnv     = "AGENT_NOTIFY_DEVIN_DESKTOP_DB"
	desktopDatabaseRel     = `devin\User\globalStorage\state.vscdb`
	desktopSessionKeyHead  = "windsurf.acp.sessioninfo.session.acp/devin-cli/"
	desktopSessionQuery    = "query_devin_desktop_session"
	desktopSessionValueCol = 1
)

// desktopSessionRecord 只解析精确回复需要的会话标识字段。
type desktopSessionRecord struct {
	Info struct {
		SessionID string `json:"sessionId"`
	} `json:"info"`
}

// LookupDesktopCascadeID 把 Devin CLI 会话号解析成 Devin 桌面端内部的 Cascade 标识。
// 桌面端把 ACP 会话登记为 acp/devin-cli/<会话号>，精确回复命令只认这个标识；
// 直接用 CLI 会话号会被桌面端判定为会话不存在。
func LookupDesktopCascadeID(sessionID string) (string, error) {
	sessionID = strings.TrimSpace(sessionID)
	if sessionID == "" {
		return "", errors.New("devin session id is empty")
	}

	path, err := desktopDatabasePath()
	if err != nil {
		return "", err
	}
	row, err := winsqlite.ReadRow(
		path,
		desktopSessionQuery,
		desktopSessionValueCol,
		"SELECT value FROM ItemTable WHERE key = ? LIMIT 1",
		desktopSessionKeyHead+sessionID,
	)
	if err != nil {
		return "", err
	}
	if !row.Found {
		return "", fmt.Errorf("%w: %s", ErrSessionNotFound, sessionID)
	}

	var record desktopSessionRecord
	if err := json.Unmarshal([]byte(row.Values[0]), &record); err != nil {
		return "", fmt.Errorf("devin desktop session %s metadata is invalid: %w", sessionID, err)
	}
	cascadeID := strings.TrimSpace(record.Info.SessionID)
	if cascadeID == "" {
		return "", fmt.Errorf("devin desktop session %s metadata has no session id", sessionID)
	}
	// 会话号对不上说明桌面端存储结构已变化，宁可直接报错也不能把回复投给别的会话。
	if !strings.HasSuffix(cascadeID, "/"+sessionID) {
		return "", fmt.Errorf(
			"devin desktop session %s maps to unexpected cascade %q",
			sessionID,
			cascadeID,
		)
	}
	return cascadeID, nil
}

func desktopDatabasePath() (string, error) {
	if path := strings.TrimSpace(os.Getenv(desktopDatabaseEnv)); path != "" {
		return path, nil
	}
	appData := strings.TrimSpace(os.Getenv("APPDATA"))
	if appData == "" {
		return "", errors.New("APPDATA is empty; cannot locate Devin desktop state database")
	}
	return filepath.Join(appData, desktopDatabaseRel), nil
}
