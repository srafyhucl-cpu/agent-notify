// Package winsqlite 提供 Windows 自带 winsqlite3.dll 的最小只读查询能力。
package winsqlite

import (
	"errors"
	"fmt"
	"sync"
	"syscall"
	"time"
	"unsafe"
)

const (
	CodeOK     = 0
	CodeError  = 1
	CodeBusy   = 5
	CodeLocked = 6
	codeRow    = 100
	codeDone   = 101

	DefaultBusyTimeout = 750 * time.Millisecond
	DefaultLockRetries = 3
	RetryDelay         = 75 * time.Millisecond
	errorTextLimit     = 4096

	openReadOnly = 0x00000001
	paramEmpty   = 0xffffffff
)

var (
	moveMemoryOnce sync.Once
	moveMemoryProc *syscall.LazyProc
	moveMemoryErr  error

	apiOnce sync.Once
	api     *sqliteAPI
	apiErr  error
)

// Error 保留 SQLite 错误码、查询阶段和重试次数，便于上层诊断。
type Error struct {
	Stage   string
	Code    int
	Retries int
	Err     error
}

func (e *Error) Error() string {
	if e == nil {
		return ""
	}
	if e.Code != 0 {
		return fmt.Sprintf("%s (sqlite code %d, retries %d): %v", e.Stage, e.Code, e.Retries, e.Err)
	}
	return fmt.Sprintf("%s (retries %d): %v", e.Stage, e.Retries, e.Err)
}

func (e *Error) Unwrap() error {
	if e == nil {
		return nil
	}
	return e.Err
}

// Row 表示一条只读查询结果。Found=false 表示查询成功但没有匹配行。
type Row struct {
	Values  []string
	Found   bool
	Retries int
}

type sqliteAPI struct {
	copyMemory  *syscall.LazyProc
	openV2      *syscall.LazyProc
	close       *syscall.LazyProc
	prepareV2   *syscall.LazyProc
	bindText    *syscall.LazyProc
	step        *syscall.LazyProc
	columnText  *syscall.LazyProc
	columnBytes *syscall.LazyProc
	finalize    *syscall.LazyProc
	errMsg      *syscall.LazyProc
	busyTimeout *syscall.LazyProc
}

// CheckAvailable 验证 winsqlite3.dll 及所需导出是否可用。
func CheckAvailable() error {
	_, err := loadAPI()
	return err
}

// ReadRow 以只读方式执行一条查询，并返回指定数量的文本列。
// 遇到 SQLite 忙锁时会按固定次数重试。
func ReadRow(path, stage string, columnCount int, query string, args ...string) (Row, error) {
	if columnCount <= 0 {
		return Row{}, &Error{Stage: stage, Err: errors.New("column count must be positive")}
	}
	sqlite, err := loadAPI()
	if err != nil {
		return Row{}, err
	}

	var lastErr error
	for attempt := 0; attempt <= DefaultLockRetries; attempt++ {
		row, queryErr := sqlite.readRow(path, stage, columnCount, query, args...)
		if queryErr == nil {
			row.Retries = attempt
			return row, nil
		}
		var sqliteErr *Error
		if !errors.As(queryErr, &sqliteErr) || !retryable(sqliteErr.Code) || attempt == DefaultLockRetries {
			if errors.As(queryErr, &sqliteErr) {
				sqliteErr.Retries = attempt
			}
			return Row{}, queryErr
		}
		lastErr = queryErr
		time.Sleep(RetryDelay)
	}
	return Row{}, lastErr
}

func (a *sqliteAPI) readRow(path, stage string, columnCount int, query string, args ...string) (Row, error) {
	db, err := a.openReadOnly(path)
	if err != nil {
		return Row{}, err
	}
	defer a.close.Call(db)

	statement, err := a.prepare(db, stage, query)
	if err != nil {
		return Row{}, err
	}
	defer a.finalize.Call(statement)

	for index, value := range args {
		valueBytes := append([]byte(value), 0)
		code, _, _ := a.bindText.Call(
			statement,
			uintptr(index+1),
			uintptr(unsafe.Pointer(&valueBytes[0])),
			paramEmpty,
			^uintptr(0),
		)
		if int(code) != CodeOK {
			return Row{}, a.sqliteError(db, stage+"_bind", uintptr(code))
		}
	}

	stepCode, _, _ := a.step.Call(statement)
	switch int(stepCode) {
	case codeRow:
		values := make([]string, columnCount)
		for index := range values {
			values[index] = a.columnString(statement, index)
		}
		return Row{Values: values, Found: true}, nil
	case codeDone:
		return Row{}, nil
	default:
		return Row{}, a.sqliteError(db, stage, stepCode)
	}
}

func (a *sqliteAPI) openReadOnly(path string) (uintptr, error) {
	var db uintptr
	pathBytes := append([]byte(path), 0)
	code, _, _ := a.openV2.Call(
		uintptr(unsafe.Pointer(&pathBytes[0])),
		uintptr(unsafe.Pointer(&db)),
		openReadOnly,
		0,
	)
	if int(code) != CodeOK {
		detail := a.errorMessage(db)
		if db != 0 {
			a.close.Call(db)
		}
		return 0, &Error{
			Stage: "open_database",
			Code:  int(code),
			Err:   errors.New(detail),
		}
	}
	a.busyTimeout.Call(db, uintptr(DefaultBusyTimeout.Milliseconds()))
	return db, nil
}

func (a *sqliteAPI) prepare(db uintptr, stage, query string) (uintptr, error) {
	var statement uintptr
	queryBytes := append([]byte(query), 0)
	code, _, _ := a.prepareV2.Call(
		db,
		uintptr(unsafe.Pointer(&queryBytes[0])),
		paramEmpty,
		uintptr(unsafe.Pointer(&statement)),
		0,
	)
	if int(code) != CodeOK || statement == 0 {
		return 0, a.sqliteError(db, stage, code)
	}
	return statement, nil
}

func (a *sqliteAPI) sqliteError(db uintptr, stage string, code uintptr) error {
	return &Error{
		Stage: stage,
		Code:  int(code),
		Err:   errors.New(a.errorMessage(db)),
	}
}

func (a *sqliteAPI) errorMessage(db uintptr) string {
	if db == 0 {
		return "sqlite database handle is unavailable"
	}
	pointer, _, _ := a.errMsg.Call(db)
	return cString(pointer)
}

func (a *sqliteAPI) columnString(statement uintptr, index int) string {
	pointer, _, _ := a.columnText.Call(statement, uintptr(index))
	if pointer == 0 {
		return ""
	}
	length, _, _ := a.columnBytes.Call(statement, uintptr(index))
	if int32(length) <= 0 {
		return ""
	}
	size := int(int32(length))
	data := make([]byte, size)
	a.copyMemory.Call(uintptr(unsafe.Pointer(&data[0])), pointer, uintptr(size))
	return string(data)
}

func retryable(code int) bool {
	return code == CodeBusy || code == CodeLocked
}

// CodeOf 返回可识别的 SQLite 错误码，其他错误返回 0。
func CodeOf(err error) int {
	var sqliteErr *Error
	if errors.As(err, &sqliteErr) {
		return sqliteErr.Code
	}
	return 0
}

func cString(pointer uintptr) string {
	if pointer == 0 {
		return ""
	}
	move, err := loadWindowsMoveMemory()
	if err != nil {
		return err.Error()
	}
	scratch := make([]byte, 1)
	data := make([]byte, 0, 64)
	for len(data) < errorTextLimit {
		move.Call(uintptr(unsafe.Pointer(&scratch[0])), pointer+uintptr(len(data)), 1)
		if scratch[0] == 0 {
			return string(data)
		}
		data = append(data, scratch[0])
	}
	return "sqlite error message exceeds diagnostic limit"
}

func loadAPI() (*sqliteAPI, error) {
	apiOnce.Do(func() {
		dll := syscall.NewLazyDLL("winsqlite3.dll")
		copyMemory, memoryErr := loadWindowsMoveMemory()
		if memoryErr != nil {
			apiErr = &Error{Stage: "load_api", Err: memoryErr}
			return
		}
		loaded := &sqliteAPI{
			copyMemory:  copyMemory,
			openV2:      dll.NewProc("sqlite3_open_v2"),
			close:       dll.NewProc("sqlite3_close"),
			prepareV2:   dll.NewProc("sqlite3_prepare_v2"),
			bindText:    dll.NewProc("sqlite3_bind_text"),
			step:        dll.NewProc("sqlite3_step"),
			columnText:  dll.NewProc("sqlite3_column_text"),
			columnBytes: dll.NewProc("sqlite3_column_bytes"),
			finalize:    dll.NewProc("sqlite3_finalize"),
			errMsg:      dll.NewProc("sqlite3_errmsg"),
			busyTimeout: dll.NewProc("sqlite3_busy_timeout"),
		}
		for name, procedure := range map[string]*syscall.LazyProc{
			"sqlite3_open_v2":      loaded.openV2,
			"sqlite3_close":        loaded.close,
			"sqlite3_prepare_v2":   loaded.prepareV2,
			"sqlite3_bind_text":    loaded.bindText,
			"sqlite3_step":         loaded.step,
			"sqlite3_column_text":  loaded.columnText,
			"sqlite3_column_bytes": loaded.columnBytes,
			"sqlite3_finalize":     loaded.finalize,
			"sqlite3_errmsg":       loaded.errMsg,
			"sqlite3_busy_timeout": loaded.busyTimeout,
		} {
			if err := procedure.Find(); err != nil {
				apiErr = &Error{
					Stage: "load_api",
					Err:   fmt.Errorf("winsqlite3.dll export %s: %w", name, err),
				}
				return
			}
		}
		api = loaded
	})
	return api, apiErr
}

// loadWindowsMoveMemory 只使用 Windows 自带的 RtlMoveMemory 复制原生指针返回的数据。
func loadWindowsMoveMemory() (*syscall.LazyProc, error) {
	moveMemoryOnce.Do(func() {
		for _, library := range []string{"kernel32.dll", "ntdll.dll"} {
			procedure := syscall.NewLazyDLL(library).NewProc("RtlMoveMemory")
			if err := procedure.Find(); err != nil {
				moveMemoryErr = fmt.Errorf("%s export RtlMoveMemory: %w", library, err)
				continue
			}
			moveMemoryProc = procedure
			moveMemoryErr = nil
			return
		}
	})
	return moveMemoryProc, moveMemoryErr
}
