package agent

import (
	"os"
	"path/filepath"
	"strconv"
	"strings"
	"syscall"
	"testing"
	"time"
	"unsafe"

	"github.com/srafyhucl-cpu/agent-notify/internal/config"
	"github.com/srafyhucl-cpu/agent-notify/internal/notify"
	"github.com/srafyhucl-cpu/agent-notify/internal/reply"
)

const (
	testSQLiteOpenReadWriteCreate = 0x00000002 | 0x00000004
	testSQLiteThreadID            = "01a0999d-1c14-77d1-99a4-3f1389429427"
)

var testSQLiteDLL = syscall.NewLazyDLL("winsqlite3.dll")

func TestDiscoverCodexStateDatabasesUsesNumericVersion(t *testing.T) {
	dir := t.TempDir()
	for _, name := range []string{"state_9.sqlite", "state_10.sqlite", "state_x.sqlite"} {
		if err := os.WriteFile(filepath.Join(dir, name), []byte("test"), 0600); err != nil {
			t.Fatalf("WriteFile(%s): %v", name, err)
		}
	}
	databases, err := discoverCodexStateDatabases(dir)
	if err != nil {
		t.Fatalf("discoverCodexStateDatabases: %v", err)
	}
	if len(databases) != 3 || filepath.Base(databases[0]) != "state_10.sqlite" {
		t.Fatalf("databases = %#v", databases)
	}
}

func TestResolveCodexTitlePrecedenceAndFallbacks(t *testing.T) {
	t.Run("threads.name wins", func(t *testing.T) {
		dir := t.TempDir()
		writeTestCodexStateDB(t, dir, 5,
			`CREATE TABLE threads (id TEXT, name TEXT, title TEXT, first_user_message TEXT);`,
			`INSERT INTO threads VALUES ('`+testSQLiteThreadID+`', '会话名', '线程标题', '第一条消息');`,
		)
		resolution := resolveCodexTitle(testSQLiteThreadID, "payload", dir)
		if resolution.Name != "会话名" || resolution.Source != codexTitleSourceName || resolution.Warning != "" {
			t.Fatalf("resolution = %#v", resolution)
		}
	})

	t.Run("threads.title is used when name is unavailable", func(t *testing.T) {
		dir := t.TempDir()
		writeTestCodexStateDB(t, dir, 5,
			`CREATE TABLE threads (id TEXT, title TEXT, first_user_message TEXT);`,
			`INSERT INTO threads VALUES ('`+testSQLiteThreadID+`', '线程标题', '第一条消息');`,
		)
		resolution := resolveCodexTitle(testSQLiteThreadID, "payload", dir)
		if resolution.Name != "线程标题" || resolution.Source != codexTitleSourceTitle {
			t.Fatalf("resolution = %#v", resolution)
		}
	})

	t.Run("first user message is used last", func(t *testing.T) {
		dir := t.TempDir()
		writeTestCodexStateDB(t, dir, 5,
			`CREATE TABLE threads (id TEXT, first_user_message TEXT);`,
			`INSERT INTO threads VALUES ('`+testSQLiteThreadID+`', '第一条消息');`,
		)
		resolution := resolveCodexTitle(testSQLiteThreadID, "payload", dir)
		if resolution.Name != "第一条消息" || resolution.Source != codexTitleSourceFirstMessage {
			t.Fatalf("resolution = %#v", resolution)
		}
	})

	t.Run("session index is used after the database", func(t *testing.T) {
		dir := t.TempDir()
		writeTestCodexStateDB(t, dir, 5,
			`CREATE TABLE threads (id TEXT, name TEXT);`,
			`INSERT INTO threads VALUES ('other-thread', '其他会话');`,
		)
		index := `{"id":"` + testSQLiteThreadID + `","thread_name":"索引会话名"}` + "\n"
		if err := os.WriteFile(filepath.Join(dir, codexSessionIndexFileName), []byte(index), 0600); err != nil {
			t.Fatal(err)
		}
		resolution := resolveCodexTitle(testSQLiteThreadID, "payload", dir)
		if resolution.Name != "索引会话名" || resolution.Source != codexTitleSourceSessionIndex {
			t.Fatalf("resolution = %#v", resolution)
		}
	})

	t.Run("payload is the final nonfatal fallback", func(t *testing.T) {
		dir := t.TempDir()
		writeTestCodexStateDB(t, dir, 5,
			`CREATE TABLE threads (id TEXT, name TEXT);`,
			`INSERT INTO threads VALUES ('other-thread', '其他会话');`,
		)
		resolution := resolveCodexTitle(testSQLiteThreadID, "payload title", dir)
		if resolution.Name != "payload title" || resolution.Source != codexTitleSourcePayload || resolution.Warning != "" {
			t.Fatalf("resolution = %#v", resolution)
		}
	})
}

func TestResolveCodexTitleUsesOlderDatabaseWhenLatestDoesNotContainThread(t *testing.T) {
	dir := t.TempDir()
	writeTestCodexStateDB(t, dir, 10,
		`CREATE TABLE threads (id TEXT, name TEXT);`,
		`INSERT INTO threads VALUES ('other-thread', '其他会话');`,
	)
	writeTestCodexStateDB(t, dir, 9,
		`CREATE TABLE threads (id TEXT, name TEXT);`,
		`INSERT INTO threads VALUES ('`+testSQLiteThreadID+`', '旧库会话');`,
	)
	resolution := resolveCodexTitle(testSQLiteThreadID, "payload", dir)
	if resolution.Name != "旧库会话" || resolution.Source != codexTitleSourceName {
		t.Fatalf("resolution = %#v", resolution)
	}
}

func TestResolveCodexTitleReportsDatabaseFailure(t *testing.T) {
	dir := t.TempDir()
	resolution := resolveCodexTitle(testSQLiteThreadID, "payload", dir)
	if resolution.Name != "payload" || resolution.Source != codexTitleSourceUnavailable {
		t.Fatalf("resolution = %#v", resolution)
	}
	if !strings.Contains(resolution.Warning, "标题读取失败") || resolution.FailureStage != "discover_database" {
		t.Fatalf("resolution = %#v", resolution)
	}
}

func TestResolveCodexTitleDoesNotWriteDatabase(t *testing.T) {
	dir := t.TempDir()
	path := writeTestCodexStateDB(t, dir, 5,
		`CREATE TABLE threads (id TEXT, name TEXT);`,
		`INSERT INTO threads VALUES ('`+testSQLiteThreadID+`', '只读会话');`,
	)
	before, err := os.ReadFile(path)
	if err != nil {
		t.Fatal(err)
	}
	entriesBefore, err := os.ReadDir(dir)
	if err != nil {
		t.Fatal(err)
	}
	resolution := resolveCodexTitle(testSQLiteThreadID, "payload", dir)
	if resolution.Name != "只读会话" {
		t.Fatalf("resolution = %#v", resolution)
	}
	after, err := os.ReadFile(path)
	if err != nil {
		t.Fatal(err)
	}
	entriesAfter, err := os.ReadDir(dir)
	if err != nil {
		t.Fatal(err)
	}
	if string(before) != string(after) || len(entriesBefore) != len(entriesAfter) {
		t.Fatalf("read-only title lookup changed database contents or directory entries")
	}
}

func TestResolveCodexTitleRetriesDuringExclusiveLock(t *testing.T) {
	dir := t.TempDir()
	path := writeTestCodexStateDB(t, dir, 5,
		`CREATE TABLE threads (id TEXT, name TEXT);`,
		`INSERT INTO threads VALUES ('`+testSQLiteThreadID+`', '锁定恢复');`,
	)
	writer := openTestSQLite(t, path)
	t.Cleanup(func() { closeTestSQLite(t, writer) })
	execTestSQLite(t, writer, "BEGIN EXCLUSIVE;")

	result := make(chan codexTitleResolution, 1)
	go func() {
		result <- resolveCodexTitle(testSQLiteThreadID, "payload", dir)
	}()
	time.Sleep(2 * codexTitleRetryDelay)
	execTestSQLite(t, writer, "COMMIT;")

	select {
	case resolution := <-result:
		if resolution.Name != "锁定恢复" || resolution.Source != codexTitleSourceName {
			t.Fatalf("resolution = %#v", resolution)
		}
	case <-time.After(5 * time.Second):
		t.Fatal("title resolution did not recover after the database lock was released")
	}
}

func TestHandleCodexUsesResolvedTitleAndKeepsReplyRoute(t *testing.T) {
	withCodexNotificationTestServer(t)
	resolver := &stubCodexTitleResolver{
		result: codexTitleResolution{
			Name:    "真实会话名",
			Source:  codexTitleSourceName,
			Warning: "标题读取失败：测试降级。",
		},
	}
	payload := `{"type":"agent-turn-complete","thread-id":"` + testSQLiteThreadID +
		`","last-assistant-message":"完成"}`
	result := handleCodex([]string{"turn-ended", payload}, resolver)
	if result.Status != "成功" {
		t.Fatalf("result = %#v", result)
	}
	if resolver.threadID != testSQLiteThreadID || resolver.payloadTitle == "" {
		t.Fatalf("resolver call = %#v", resolver)
	}
	route, err := reply.NewRouteStore("").Find("bot-1", "user-1", "platform-1", "")
	if err != nil {
		t.Fatal(err)
	}
	if route.SessionID != testSQLiteThreadID {
		t.Fatalf("route = %#v", route)
	}
	history, err := notify.GetHistory(1, "")
	if err != nil || len(history) != 1 {
		t.Fatalf("history = %#v, err=%v", history, err)
	}
	if history[0].Title != "【codex】真实会话名" || !strings.Contains(history[0].Summary, "测试降级") {
		t.Fatalf("history = %#v", history[0])
	}
}

type stubCodexTitleResolver struct {
	result       codexTitleResolution
	threadID     string
	payloadTitle string
}

func (r *stubCodexTitleResolver) Resolve(threadID, payloadTitle string) codexTitleResolution {
	r.threadID = threadID
	r.payloadTitle = payloadTitle
	return r.result
}

func TestResolveCodexTitleReadsWalDatabase(t *testing.T) {
	dir := t.TempDir()
	path := filepath.Join(dir, "state_5.sqlite")
	writer := openTestSQLite(t, path)
	t.Cleanup(func() { closeTestSQLite(t, writer) })
	execTestSQLite(t, writer, "PRAGMA journal_mode=WAL;")
	execTestSQLite(t, writer, `CREATE TABLE threads (id TEXT, name TEXT);`)
	execTestSQLite(t, writer, `INSERT INTO threads VALUES ('`+testSQLiteThreadID+`', 'WAL 会话');`)

	resolution := resolveCodexTitle(testSQLiteThreadID, "payload", dir)
	if resolution.Name != "WAL 会话" || resolution.Source != codexTitleSourceName {
		t.Fatalf("resolution = %#v", resolution)
	}
}

func TestCheckCodexTitleHealth(t *testing.T) {
	t.Run("normal", func(t *testing.T) {
		dir := t.TempDir()
		writeTestCodexStateDB(t, dir, 5,
			`CREATE TABLE threads (id TEXT, name TEXT, title TEXT, first_user_message TEXT);`,
			`INSERT INTO threads VALUES ('`+testSQLiteThreadID+`', '健康会话', '', '');`,
		)
		t.Setenv("CODEX_HOME", dir)
		health := CheckCodexTitleHealth()
		if health.Status != CodexTitleStatusNormal || !strings.Contains(health.Detail, "threads.name") {
			t.Fatalf("health = %#v", health)
		}
	})

	t.Run("missing database", func(t *testing.T) {
		t.Setenv("CODEX_HOME", t.TempDir())
		health := CheckCodexTitleHealth()
		if health.Status != CodexTitleStatusFailed {
			t.Fatalf("health = %#v", health)
		}
	})

	t.Run("empty threads table degrades", func(t *testing.T) {
		dir := t.TempDir()
		writeTestCodexStateDB(t, dir, 5,
			`CREATE TABLE threads (id TEXT, name TEXT, title TEXT, first_user_message TEXT);`,
		)
		t.Setenv("CODEX_HOME", dir)
		health := CheckCodexTitleHealth()
		if health.Status != CodexTitleStatusDegraded {
			t.Fatalf("health = %#v", health)
		}
	})
}

func TestWriteCodexTitleDiagnostic(t *testing.T) {
	configDir := t.TempDir()
	tempDir := t.TempDir()
	t.Setenv("AGENT_NOTIFY_CONFIG_DIR", configDir)
	t.Setenv("AGENT_NOTIFY_TEMP_DIR", tempDir)
	writeCodexTitleDiagnostic("thread-1", codexTitleResolution{
		Name:           "不应写入诊断的标题",
		Source:         codexTitleSourceUnavailable,
		Warning:        "标题读取失败",
		FailureStage:   "open_database",
		FailureDetail:  "sqlite code 5",
		FallbackSource: codexTitleSourcePayload,
		SQLiteCode:     sqliteBusy,
		Retries:        3,
	})
	data, err := os.ReadFile(config.GetPaths().CodexTitleLog)
	if err != nil {
		t.Fatal(err)
	}
	line := string(data)
	for _, want := range []string{"thread-1", codexTitleSourceUnavailable, "open_database", "sqlite code 5"} {
		if !strings.Contains(line, want) {
			t.Fatalf("diagnostic = %q, missing %q", line, want)
		}
	}
	if strings.Contains(line, "不应写入诊断的标题") {
		t.Fatalf("diagnostic unexpectedly contains title text: %q", line)
	}
}

func writeTestCodexStateDB(t *testing.T, dir string, version int, statements ...string) string {
	t.Helper()
	path := filepath.Join(dir, "state_"+strconv.Itoa(version)+".sqlite")
	db := openTestSQLite(t, path)
	defer closeTestSQLite(t, db)
	for _, statement := range statements {
		execTestSQLite(t, db, statement)
	}
	return path
}

func openTestSQLite(t *testing.T, path string) uintptr {
	t.Helper()
	for _, name := range []string{"sqlite3_open_v2", "sqlite3_exec", "sqlite3_close", "sqlite3_free"} {
		if err := testSQLiteDLL.NewProc(name).Find(); err != nil {
			t.Fatalf("winsqlite3.dll export %s: %v", name, err)
		}
	}
	var db uintptr
	pathBytes := append([]byte(path), 0)
	code, _, _ := testSQLiteDLL.NewProc("sqlite3_open_v2").Call(
		uintptr(unsafe.Pointer(&pathBytes[0])),
		uintptr(unsafe.Pointer(&db)),
		testSQLiteOpenReadWriteCreate,
		0,
	)
	if int(code) != sqliteOK || db == 0 {
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
	if int(code) == sqliteOK {
		return
	}
	detail := testSQLiteCString(errorMessage)
	if errorMessage != 0 {
		testSQLiteDLL.NewProc("sqlite3_free").Call(errorMessage)
	}
	t.Fatalf("sqlite exec %q code=%d detail=%s", statement, code, detail)
}

func closeTestSQLite(t *testing.T, db uintptr) {
	t.Helper()
	if db == 0 {
		return
	}
	if code, _, _ := testSQLiteDLL.NewProc("sqlite3_close").Call(db); int(code) != sqliteOK {
		t.Fatalf("sqlite3_close code=%d", code)
	}
}

func testSQLiteCString(pointer uintptr) string {
	if pointer == 0 {
		return ""
	}
	move := syscall.NewLazyDLL("kernel32.dll").NewProc("RtlMoveMemory")
	data := make([]byte, 0, 64)
	for len(data) < 4096 {
		var value byte
		move.Call(uintptr(unsafe.Pointer(&value)), pointer+uintptr(len(data)), 1)
		if value == 0 {
			return string(data)
		}
		data = append(data, value)
	}
	return "sqlite error message exceeds diagnostic limit"
}
