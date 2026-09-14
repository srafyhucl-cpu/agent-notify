package devinsession

import (
	"errors"
	"path/filepath"
	"strings"
	"testing"
)

func TestLookupDesktopCascadeIDUsesDesktopSessionID(t *testing.T) {
	path := writeDesktopDatabase(t,
		`INSERT INTO ItemTable (key, value) VALUES (`+
			`'windsurf.acp.sessioninfo.session.acp/devin-cli/complete-tourmaline', `+
			`'{"providerId":"devin-cli","info":{"sessionId":"acp/devin-cli/complete-tourmaline","title":"Hello"}}');`,
	)
	t.Setenv(desktopDatabaseEnv, path)

	cascadeID, err := LookupDesktopCascadeID("complete-tourmaline")
	if err != nil {
		t.Fatalf("LookupDesktopCascadeID: %v", err)
	}
	if cascadeID != "acp/devin-cli/complete-tourmaline" {
		t.Fatalf("cascadeID = %q", cascadeID)
	}
}

func TestLookupDesktopCascadeIDRejectsUnknownSession(t *testing.T) {
	path := writeDesktopDatabase(t)
	t.Setenv(desktopDatabaseEnv, path)

	_, err := LookupDesktopCascadeID("missing-session")
	if !errors.Is(err, ErrSessionNotFound) {
		t.Fatalf("error = %v, want ErrSessionNotFound", err)
	}
}

func TestLookupDesktopCascadeIDRejectsMismatchedRecord(t *testing.T) {
	path := writeDesktopDatabase(t,
		`INSERT INTO ItemTable (key, value) VALUES (`+
			`'windsurf.acp.sessioninfo.session.acp/devin-cli/session-1', `+
			`'{"info":{"sessionId":"acp/devin-cli/another-session"}}');`,
	)
	t.Setenv(desktopDatabaseEnv, path)

	if _, err := LookupDesktopCascadeID("session-1"); err == nil ||
		!strings.Contains(err.Error(), "unexpected cascade") {
		t.Fatalf("error = %v, want unexpected-cascade error", err)
	}
}

func writeDesktopDatabase(t *testing.T, statements ...string) string {
	t.Helper()
	path := filepath.Join(t.TempDir(), "state.vscdb")
	db := openTestSQLite(t, path)
	defer closeTestSQLite(t, db)
	execTestSQLite(t, db, "CREATE TABLE ItemTable (key TEXT PRIMARY KEY, value BLOB);")
	for _, statement := range statements {
		execTestSQLite(t, db, statement)
	}
	return path
}
