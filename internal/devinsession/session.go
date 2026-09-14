// Package devinsession 读取 Devin CLI 的本地会话元数据。
package devinsession

import (
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"strings"

	"github.com/srafyhucl-cpu/agent-notify/internal/winsqlite"
)

const (
	sessionsDatabaseEnv = "AGENT_NOTIFY_DEVIN_SESSIONS_DB"
	sessionsDatabaseRel = `devin\cli\sessions.db`
)

var ErrSessionNotFound = errors.New("devin session not found")

type Session struct {
	ID               string
	Title            string
	WorkingDirectory string
}

// LookupSession 按完整 session ID 读取标题和工作目录，不接受模糊匹配。
func LookupSession(sessionID string) (Session, error) {
	sessionID = strings.TrimSpace(sessionID)
	if sessionID == "" {
		return Session{}, errors.New("devin session id is empty")
	}

	path, err := databasePath()
	if err != nil {
		return Session{}, err
	}
	row, err := winsqlite.ReadRow(
		path,
		"query_devin_session",
		3,
		"SELECT id, title, working_directory FROM sessions WHERE id = ? LIMIT 1",
		sessionID,
	)
	if err != nil {
		return Session{}, err
	}
	if !row.Found {
		return Session{}, fmt.Errorf("%w: %s", ErrSessionNotFound, sessionID)
	}

	session := Session{
		ID:               strings.TrimSpace(row.Values[0]),
		Title:            cleanTitle(row.Values[1]),
		WorkingDirectory: strings.TrimSpace(row.Values[2]),
	}
	if session.ID != sessionID {
		return Session{}, fmt.Errorf("devin session lookup returned %q for %q", session.ID, sessionID)
	}
	return session, nil
}

func databasePath() (string, error) {
	if path := strings.TrimSpace(os.Getenv(sessionsDatabaseEnv)); path != "" {
		return path, nil
	}
	appData := strings.TrimSpace(os.Getenv("APPDATA"))
	if appData == "" {
		return "", errors.New("APPDATA is empty; cannot locate Devin sessions.db")
	}
	return filepath.Join(appData, sessionsDatabaseRel), nil
}

func cleanTitle(value string) string {
	return strings.Join(strings.Fields(value), " ")
}
