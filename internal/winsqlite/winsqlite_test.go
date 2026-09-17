//go:build windows

package winsqlite

import (
	"bytes"
	"errors"
	"fmt"
	"path/filepath"
	"runtime"
	"strings"
	"testing"
	"time"
	"unsafe"
)

// 测试专用常量：只读/读写打开标志（SQLite C API 定义）与 CANTOPEN 错误码。
const (
	testOpenReadWrite = 0x00000002
	testOpenCreate    = 0x00000004
	testCodeCantOpen  = 14
)

// requireSQLite 在 winsqlite3.dll 不可用时跳过用例，避免环境差异导致失败。
func requireSQLite(t *testing.T) *sqliteAPI {
	t.Helper()
	if err := CheckAvailable(); err != nil {
		t.Skipf("当前环境无法加载 winsqlite3.dll：%v", err)
	}
	sqlite, err := loadAPI()
	if err != nil {
		t.Fatalf("loadAPI 失败：%v", err)
	}
	return sqlite
}

// openWriteConnection 直接通过 DLL 以读写+创建方式打开数据库，用于准备真实测试夹具。
func openWriteConnection(t *testing.T, sqlite *sqliteAPI, path string) uintptr {
	t.Helper()
	pathBytes := append([]byte(path), 0)
	var db uintptr
	code, _, _ := sqlite.openV2.Call(
		uintptr(unsafe.Pointer(&pathBytes[0])),
		uintptr(unsafe.Pointer(&db)),
		uintptr(testOpenReadWrite|testOpenCreate),
		0,
	)
	if int(code) != CodeOK {
		t.Fatalf("以读写方式打开 %s 失败：code=%d msg=%s", path, code, sqlite.errorMessage(db))
	}
	return db
}

// execStatement 执行一条写语句，仅用于构造夹具，生产代码不依赖此路径。
func execStatement(sqlite *sqliteAPI, db uintptr, statement string) error {
	prepared, err := sqlite.prepare(db, "test_fixture", statement)
	if err != nil {
		return err
	}
	defer sqlite.finalize.Call(prepared)
	code, _, _ := sqlite.step.Call(prepared)
	if int(code) != codeDone {
		return sqlite.sqliteError(db, "test_fixture", code)
	}
	return nil
}

func createDatabase(t *testing.T, path string, statements ...string) {
	t.Helper()
	sqlite := requireSQLite(t)
	db := openWriteConnection(t, sqlite, path)
	defer sqlite.close.Call(db)
	for _, statement := range statements {
		if err := execStatement(sqlite, db, statement); err != nil {
			t.Fatalf("初始化测试数据库失败 %q：%v", statement, err)
		}
	}
}

// newFixtureDatabase 生成含正常值、NULL、空串与多字节文本的真实 SQLite 文件。
func newFixtureDatabase(t *testing.T) string {
	t.Helper()
	path := filepath.Join(t.TempDir(), "fixture.db")
	createDatabase(t, path,
		"CREATE TABLE items(id INTEGER PRIMARY KEY, name TEXT, note TEXT);",
		"INSERT INTO items(id, name, note) VALUES(1, 'alice', 'first');",
		"INSERT INTO items(id, name, note) VALUES(2, '中文名字', NULL);",
		"INSERT INTO items(id, name, note) VALUES(3, '', 'empty');",
	)
	return path
}

func TestReadRowReturnsMatchingRow(t *testing.T) {
	requireSQLite(t)
	path := newFixtureDatabase(t)

	row, err := ReadRow(path, "test_query", 2, "SELECT name, note FROM items WHERE id = ? LIMIT 1", "1")
	if err != nil {
		t.Fatalf("查询应成功，实际错误：%v", err)
	}
	if !row.Found {
		t.Fatal("查询应命中一行，Found=false")
	}
	if len(row.Values) != 2 {
		t.Fatalf("列数应为 2，实际 %d：%v", len(row.Values), row.Values)
	}
	if row.Values[0] != "alice" || row.Values[1] != "first" {
		t.Fatalf("查询结果不符：%v", row.Values)
	}
	if row.Retries != 0 {
		t.Fatalf("无锁竞争时不应重试，实际 Retries=%d", row.Retries)
	}
}

func TestReadRowEmptyResult(t *testing.T) {
	requireSQLite(t)
	path := newFixtureDatabase(t)

	row, err := ReadRow(path, "test_empty", 1, "SELECT name FROM items WHERE id = ? LIMIT 1", "999")
	if err != nil {
		t.Fatalf("空结果不应报错，实际错误：%v", err)
	}
	if row.Found {
		t.Fatalf("空结果 Found 应为 false：%v", row.Values)
	}
	if len(row.Values) != 0 {
		t.Fatalf("空结果不应返回值：%v", row.Values)
	}
}

func TestReadRowNullAndEmptyColumns(t *testing.T) {
	requireSQLite(t)
	path := newFixtureDatabase(t)

	// NULL 列与空串列在读取层都应表现为 ""。
	row, err := ReadRow(path, "test_null", 2, "SELECT name, note FROM items WHERE id = ?", "2")
	if err != nil {
		t.Fatalf("读取 NULL 行失败：%v", err)
	}
	if !row.Found || len(row.Values) != 2 {
		t.Fatalf("读取 NULL 行结果异常：%+v", row)
	}
	if row.Values[0] != "中文名字" {
		t.Fatalf("多字节文本读取错误：%q", row.Values[0])
	}
	if row.Values[1] != "" {
		t.Fatalf("NULL 列应为空串，实际 %q", row.Values[1])
	}

	row, err = ReadRow(path, "test_zero_len", 2, "SELECT name, note FROM items WHERE id = ?", "3")
	if err != nil {
		t.Fatalf("读取空串行失败：%v", err)
	}
	if row.Values[0] != "" || row.Values[1] != "empty" {
		t.Fatalf("零长度文本读取错误：%v", row.Values)
	}
}

func TestReadRowLongText(t *testing.T) {
	requireSQLite(t)
	path := filepath.Join(t.TempDir(), "long.db")
	long := strings.Repeat("汉", 4000) // 12000 字节 UTF-8，覆盖 copyMemory 长度拷贝路径。
	createDatabase(t, path,
		"CREATE TABLE items(name TEXT);",
		fmt.Sprintf("INSERT INTO items(name) VALUES('%s');", long),
	)

	row, err := ReadRow(path, "test_long", 1, "SELECT name FROM items LIMIT 1")
	if err != nil {
		t.Fatalf("读取长文本失败：%v", err)
	}
	if !row.Found || row.Values[0] != long {
		t.Fatalf("长文本读取不一致：len(got)=%d len(want)=%d", len(row.Values[0]), len(long))
	}
}

func TestReadRowMultipleArgs(t *testing.T) {
	requireSQLite(t)
	path := newFixtureDatabase(t)

	row, err := ReadRow(path, "test_multi_args", 1, "SELECT note FROM items WHERE id = ? AND name = ?", "1", "alice")
	if err != nil {
		t.Fatalf("多参数绑定失败：%v", err)
	}
	if !row.Found || row.Values[0] != "first" {
		t.Fatalf("多参数绑定结果不符：%+v", row)
	}
}

func TestReadRowSyntaxError(t *testing.T) {
	requireSQLite(t)
	path := newFixtureDatabase(t)

	row, err := ReadRow(path, "test_bad_sql", 1, "SELECT FROM items")
	if err == nil {
		t.Fatal("语法错误应返回 error")
	}
	if row.Found || len(row.Values) != 0 {
		t.Fatalf("出错时不应返回行：%+v", row)
	}
	if code := CodeOf(err); code != CodeError {
		t.Fatalf("语法错误码应为 %d，实际 %d（%v）", CodeError, code, err)
	}
	message := err.Error()
	if !strings.Contains(message, "test_bad_sql") || !strings.Contains(message, "sqlite code 1") {
		t.Fatalf("错误信息缺少阶段或错误码：%q", message)
	}
	if !strings.Contains(strings.ToLower(message), "syntax error") {
		t.Fatalf("错误信息不可读：%q", message)
	}
}

func TestReadRowInvalidColumnCount(t *testing.T) {
	for _, columnCount := range []int{0, -1} {
		row, err := ReadRow("ignored.db", "test_column_count", columnCount, "SELECT 1")
		if err == nil {
			t.Fatalf("columnCount=%d 应返回 error", columnCount)
		}
		if row.Found || len(row.Values) != 0 {
			t.Fatalf("columnCount=%d 出错时不应返回行：%+v", columnCount, row)
		}
		if CodeOf(err) != CodeOK {
			t.Fatalf("columnCount=%d 不应带 SQLite 错误码：%v", columnCount, err)
		}
		message := err.Error()
		if !strings.Contains(message, "test_column_count") || !strings.Contains(message, "column count must be positive") {
			t.Fatalf("columnCount=%d 错误信息不清晰：%q", columnCount, message)
		}
	}
}

func TestReadRowOpenFailures(t *testing.T) {
	requireSQLite(t)
	tempDir := t.TempDir()
	cases := []struct {
		name string
		path string
	}{
		{"文件不存在", filepath.Join(tempDir, "missing.db")},
		{"父目录不存在", filepath.Join(tempDir, "no_such_dir", "missing.db")},
		{"路径是目录", tempDir},
	}
	for _, testCase := range cases {
		t.Run(testCase.name, func(t *testing.T) {
			row, err := ReadRow(testCase.path, "test_open_failure", 1, "SELECT 1")
			if err == nil {
				t.Fatal("打开失败应返回 error")
			}
			if row.Found || len(row.Values) != 0 {
				t.Fatalf("打开失败时不应返回行：%+v", row)
			}
			if code := CodeOf(err); code != testCodeCantOpen {
				t.Fatalf("应为 CANTOPEN(%d)，实际 %d（%v）", testCodeCantOpen, code, err)
			}
			var sqliteErr *Error
			if !errors.As(err, &sqliteErr) {
				t.Fatalf("错误应可 As 到 *Error：%v", err)
			}
			if sqliteErr.Stage != "open_database" {
				t.Fatalf("阶段应为 open_database，实际 %q", sqliteErr.Stage)
			}
			if message := strings.ToLower(err.Error()); !strings.Contains(message, "unable to open") {
				t.Fatalf("打开失败信息不可读：%q", err.Error())
			}
		})
	}
}

// TestReadRowBusyLock 用另一连接持有 BEGIN EXCLUSIVE 制造排他锁。
// 只读查询会走满重试并最终失败，属于确定性路径，不依赖 sleep 赌时序。
func TestReadRowBusyLock(t *testing.T) {
	sqlite := requireSQLite(t)
	path := newFixtureDatabase(t)

	writer := openWriteConnection(t, sqlite, path)
	t.Cleanup(func() {
		_ = execStatement(sqlite, writer, "ROLLBACK;")
		sqlite.close.Call(writer)
	})
	if err := execStatement(sqlite, writer, "BEGIN EXCLUSIVE;"); err != nil {
		t.Fatalf("获取排他锁失败：%v", err)
	}

	start := time.Now()
	row, err := ReadRow(path, "test_busy", 1, "SELECT name FROM items WHERE id = ?", "1")
	elapsed := time.Since(start)

	if err == nil {
		t.Fatal("持有排他锁时读取应失败")
	}
	if row.Found || len(row.Values) != 0 {
		t.Fatalf("忙锁失败时不应返回行：%+v", row)
	}
	code := CodeOf(err)
	if code != CodeBusy && code != CodeLocked {
		t.Fatalf("应为 BUSY(%d) 或 LOCKED(%d)，实际 %d（%v）", CodeBusy, CodeLocked, code, err)
	}
	if !retryable(code) {
		t.Fatalf("错误码 %d 应被判定为可重试", code)
	}
	var sqliteErr *Error
	if !errors.As(err, &sqliteErr) {
		t.Fatalf("错误应可 As 到 *Error：%v", err)
	}
	if sqliteErr.Retries != DefaultLockRetries {
		t.Fatalf("应耗尽重试次数 %d，实际 %d", DefaultLockRetries, sqliteErr.Retries)
	}
	// 重试之间有固定 sleep，用其下限确认确实发生了重试而不是一次即失败。
	if minimum := time.Duration(DefaultLockRetries) * RetryDelay; elapsed < minimum {
		t.Fatalf("重试耗时过短，疑似未重试：%v < %v", elapsed, minimum)
	}

	// 释放锁后同一查询应恢复成功，证明前一次失败确实来自锁竞争。
	if err := execStatement(sqlite, writer, "ROLLBACK;"); err != nil {
		t.Fatalf("释放排他锁失败：%v", err)
	}
	row, err = ReadRow(path, "test_busy_released", 1, "SELECT name FROM items WHERE id = ?", "1")
	if err != nil {
		t.Fatalf("释放锁后应能读取：%v", err)
	}
	if !row.Found || row.Values[0] != "alice" {
		t.Fatalf("释放锁后读取结果不符：%+v", row)
	}
}

func TestRetryable(t *testing.T) {
	cases := []struct {
		name string
		code int
		want bool
	}{
		{"busy", CodeBusy, true},
		{"locked", CodeLocked, true},
		{"ok", CodeOK, false},
		{"error", CodeError, false},
		{"row", codeRow, false},
		{"done", codeDone, false},
		{"unknown", 42, false},
	}
	for _, testCase := range cases {
		t.Run(testCase.name, func(t *testing.T) {
			if got := retryable(testCase.code); got != testCase.want {
				t.Fatalf("retryable(%d) = %v，期望 %v", testCase.code, got, testCase.want)
			}
		})
	}
}

func TestCodeOf(t *testing.T) {
	base := errors.New("底层错误")
	cases := []struct {
		name string
		err  error
		want int
	}{
		{"nil", nil, 0},
		{"普通错误", errors.New("plain"), 0},
		{"busy", &Error{Code: CodeBusy}, CodeBusy},
		{"locked", &Error{Code: CodeLocked}, CodeLocked},
		{"零错误码", &Error{Err: base}, 0},
		{"单层包装", fmt.Errorf("wrap: %w", &Error{Code: CodeBusy}), CodeBusy},
		{"多层包装", fmt.Errorf("outer: %w", fmt.Errorf("inner: %w", &Error{Code: 42})), 42},
	}
	for _, testCase := range cases {
		t.Run(testCase.name, func(t *testing.T) {
			if got := CodeOf(testCase.err); got != testCase.want {
				t.Fatalf("CodeOf(%v) = %d，期望 %d", testCase.err, got, testCase.want)
			}
		})
	}
}

func TestErrorUnwrapAndIs(t *testing.T) {
	base := errors.New("底层错误")
	err := fmt.Errorf("外层: %w", &Error{Stage: "stage_x", Code: CodeBusy, Err: base})

	if !errors.Is(err, base) {
		t.Fatal("errors.Is 应能穿透 Error.Unwrap 找到底层错误")
	}
	var sqliteErr *Error
	if !errors.As(err, &sqliteErr) {
		t.Fatal("errors.As 应能找到 *Error")
	}
	if sqliteErr.Code != CodeBusy || sqliteErr.Stage != "stage_x" {
		t.Fatalf("As 结果不符：%+v", sqliteErr)
	}
}

func TestErrorFormatting(t *testing.T) {
	base := errors.New("some detail")

	withCode := &Error{Stage: "stage_x", Code: CodeBusy, Retries: 2, Err: base}
	message := withCode.Error()
	for _, fragment := range []string{"stage_x", "sqlite code 5", "retries 2", "some detail"} {
		if !strings.Contains(message, fragment) {
			t.Fatalf("带错误码信息缺少 %q：%q", fragment, message)
		}
	}

	withoutCode := &Error{Stage: "stage_y", Retries: 1, Err: base}
	message = withoutCode.Error()
	for _, fragment := range []string{"stage_y", "retries 1", "some detail"} {
		if !strings.Contains(message, fragment) {
			t.Fatalf("无错误码信息缺少 %q：%q", fragment, message)
		}
	}
	if strings.Contains(message, "sqlite code") {
		t.Fatalf("错误码为 0 时不应打印 sqlite code：%q", message)
	}

	var nilErr *Error
	if got := nilErr.Error(); got != "" {
		t.Fatalf("nil Error.Error() 应为空串，实际 %q", got)
	}
	if got := nilErr.Unwrap(); got != nil {
		t.Fatalf("nil Error.Unwrap() 应为 nil，实际 %v", got)
	}
	if withCode.Unwrap() != base {
		t.Fatal("Unwrap 应返回底层错误")
	}
}

func TestCString(t *testing.T) {
	t.Run("空指针", func(t *testing.T) {
		if got := cString(0); got != "" {
			t.Fatalf("cString(0) 应为空串，实际 %q", got)
		}
	})

	t.Run("NUL 结尾", func(t *testing.T) {
		buffer := append([]byte("hello"), 0)
		got := cString(uintptr(unsafe.Pointer(&buffer[0])))
		runtime.KeepAlive(buffer)
		if got != "hello" {
			t.Fatalf("cString 读取错误：%q", got)
		}
	})

	t.Run("空字符串", func(t *testing.T) {
		buffer := []byte{0}
		got := cString(uintptr(unsafe.Pointer(&buffer[0])))
		runtime.KeepAlive(buffer)
		if got != "" {
			t.Fatalf("空字符串应返回空串，实际 %q", got)
		}
	})

	t.Run("超过长度上限", func(t *testing.T) {
		buffer := bytes.Repeat([]byte{'A'}, errorTextLimit)
		got := cString(uintptr(unsafe.Pointer(&buffer[0])))
		runtime.KeepAlive(buffer)
		if got != "sqlite error message exceeds diagnostic limit" {
			t.Fatalf("超长输入应返回截断提示，实际 len=%d %q", len(got), got)
		}
	})
}
