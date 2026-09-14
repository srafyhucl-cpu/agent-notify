package devinsession

import (
	"errors"
	"path/filepath"
	"syscall"
	"testing"
	"unsafe"
)

const testSQLiteOpenReadWriteCreate = 0x00000002 | 0x00000004

var testSQLiteDLL = syscall.NewLazyDLL("winsqlite3.dll")

func TestLookupSessionReadsExactMetadata(t *testing.T) {
	path := writeSessionDatabase(t,
		`INSERT INTO sessions (id, title, working_directory) VALUES ('righteous-peace', '  Greeting   and Conversation Start  ', 'D:\Project\yueyou');`,
		`INSERT INTO sessions (id, title, working_directory) VALUES ('other-session', 'Other', 'D:\Project\other');`,
	)
	t.Setenv(sessionsDatabaseEnv, path)

	session, err := LookupSession("righteous-peace")
	if err != nil {
		t.Fatalf("LookupSession: %v", err)
	}
	if session.ID != "righteous-peace" ||
		session.Title != "Greeting and Conversation Start" ||
		session.WorkingDirectory != `D:\Project\yueyou` {
		t.Fatalf("session = %#v", session)
	}

	if _, err := LookupSession("righteous"); !errors.Is(err, ErrSessionNotFound) {
		t.Fatalf("partial session lookup error = %v, want ErrSessionNotFound", err)
	}
}

func writeSessionDatabase(t *testing.T, statements ...string) string {
	t.Helper()
	path := filepath.Join(t.TempDir(), "sessions.db")
	db := openTestSQLite(t, path)
	defer closeTestSQLite(t, db)
	for _, statement := range []string{
		"CREATE TABLE sessions (id TEXT NOT NULL, title TEXT, working_directory TEXT);",
	} {
		execTestSQLite(t, db, statement)
	}
	for _, statement := range statements {
		execTestSQLite(t, db, statement)
	}
	return path
}

func openTestSQLite(t *testing.T, path string) uintptr {
	t.Helper()
	var db uintptr
	pathBytes := append([]byte(path), 0)
	code, _, _ := testSQLiteDLL.NewProc("sqlite3_open_v2").Call(
		uintptr(unsafe.Pointer(&pathBytes[0])),
		uintptr(unsafe.Pointer(&db)),
		testSQLiteOpenReadWriteCreate,
		0,
	)
	if int(code) != 0 || db == 0 {
		t.Fatalf("sqlite3_open_v2(%s) code=%d", path, code)
	}
	return db
}

func execTestSQLite(t *testing.T, db uintptr, statement string) {
	t.Helper()
	statementBytes := append([]byte(statement), 0)
	var errorMessage uintptr
	code, _, _ := testSQLiteDLL.NewProc("sqlite3_exec").Call(
		db,
		uintptr(unsafe.Pointer(&statementBytes[0])),
		0,
		0,
		uintptr(unsafe.Pointer(&errorMessage)),
	)
	if int(code) == 0 {
		return
	}
	if errorMessage != 0 {
		testSQLiteDLL.NewProc("sqlite3_free").Call(errorMessage)
	}
	t.Fatalf("sqlite exec %q code=%d", statement, code)
}

func closeTestSQLite(t *testing.T, db uintptr) {
	t.Helper()
	if db == 0 {
		return
	}
	if code, _, _ := testSQLiteDLL.NewProc("sqlite3_close").Call(db); int(code) != 0 {
		t.Fatalf("sqlite3_close code=%d", code)
	}
}
