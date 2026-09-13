package agent

import (
	"errors"
	"fmt"
	"sync"
	"syscall"
	"time"
	"unsafe"
)

const (
	sqliteOK         = 0
	sqliteError      = 1
	sqliteRow        = 100
	sqliteDone       = 101
	sqliteBusy       = 5
	sqliteLocked     = 6
	sqliteOpenRead   = 0x00000001
	sqliteParamEmpty = 0xffffffff

	codexTitleBusyTimeout = 750 * time.Millisecond
	codexTitleLockRetries = 3
	codexTitleRetryDelay  = 75 * time.Millisecond
	codexTitleErrorLimit  = 4096
)

var (
	winMoveMemoryOnce sync.Once
	winMoveMemoryProc *syscall.LazyProc
	winMoveMemoryErr  error

	codexSQLiteOnce sync.Once
	codexSQLite     *codexSQLiteAPI
	codexSQLiteErr  error
)

type codexSQLiteAPI struct {
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

type codexTitleLookup struct {
	Name    string
	Source  string
	Retries int
}

type codexSQLiteError struct {
	Stage   string
	Code    int
	Retries int
	Err     error
}

func (e *codexSQLiteError) Error() string {
	if e == nil {
		return ""
	}
	if e.Code != 0 {
		return fmt.Sprintf("%s (sqlite code %d, retries %d): %v", e.Stage, e.Code, e.Retries, e.Err)
	}
	return fmt.Sprintf("%s (retries %d): %v", e.Stage, e.Retries, e.Err)
}

func (e *codexSQLiteError) Unwrap() error {
	if e == nil {
		return nil
	}
	return e.Err
}

func (e *codexSQLiteError) retryable() bool {
	return e.Code == sqliteBusy || e.Code == sqliteLocked
}

func loadCodexSQLiteAPI() (*codexSQLiteAPI, error) {
	codexSQLiteOnce.Do(func() {
		dll := syscall.NewLazyDLL("winsqlite3.dll")
		copyMemory, memoryErr := loadWindowsMoveMemory()
		if memoryErr != nil {
			codexSQLiteErr = memoryErr
			return
		}
		api := &codexSQLiteAPI{
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
			"sqlite3_open_v2":      api.openV2,
			"sqlite3_close":        api.close,
			"sqlite3_prepare_v2":   api.prepareV2,
			"sqlite3_bind_text":    api.bindText,
			"sqlite3_step":         api.step,
			"sqlite3_column_text":  api.columnText,
			"sqlite3_column_bytes": api.columnBytes,
			"sqlite3_finalize":     api.finalize,
			"sqlite3_errmsg":       api.errMsg,
			"sqlite3_busy_timeout": api.busyTimeout,
		} {
			if err := procedure.Find(); err != nil {
				codexSQLiteErr = fmt.Errorf("winsqlite3.dll export %s: %w", name, err)
				return
			}
		}
		codexSQLite = api
	})
	return codexSQLite, codexSQLiteErr
}

func queryCodexTitleDatabase(path, threadID string) (codexTitleLookup, error) {
	api, err := loadCodexSQLiteAPI()
	if err != nil {
		return codexTitleLookup{}, err
	}

	var lastErr error
	for attempt := 0; attempt <= codexTitleLockRetries; attempt++ {
		lookup, queryErr := api.queryTitle(path, threadID)
		if queryErr == nil {
			lookup.Retries = attempt
			return lookup, nil
		}
		var sqliteErr *codexSQLiteError
		if !errors.As(queryErr, &sqliteErr) || !sqliteErr.retryable() || attempt == codexTitleLockRetries {
			sqliteErr.Retries = attempt
			return codexTitleLookup{}, sqliteErr
		}
		lastErr = queryErr
		time.Sleep(codexTitleRetryDelay)
	}
	return codexTitleLookup{}, lastErr
}

func (a *codexSQLiteAPI) queryTitle(path, threadID string) (codexTitleLookup, error) {
	db, err := a.openReadOnly(path)
	if err != nil {
		return codexTitleLookup{}, err
	}
	defer a.close.Call(db)

	columns := []struct {
		name   string
		source string
	}{
		{name: "name", source: codexTitleSourceName},
		{name: "title", source: codexTitleSourceTitle},
		{name: "first_user_message", source: codexTitleSourceFirstMessage},
	}
	var lastErr error
	queryable := false
	for _, column := range columns {
		name, lookupErr := a.queryColumn(db, column.name, threadID)
		if lookupErr != nil {
			if errors.Is(lookupErr, errSQLiteColumnUnavailable) {
				lastErr = lookupErr
				continue
			}
			return codexTitleLookup{}, lookupErr
		}
		queryable = true
		if name != "" {
			return codexTitleLookup{Name: name, Source: column.source}, nil
		}
	}
	if !queryable {
		if lastErr != nil {
			return codexTitleLookup{}, lastErr
		}
		return codexTitleLookup{}, &codexSQLiteError{
			Stage: "prepare_threads_columns",
			Err:   errors.New("threads table has no compatible title column"),
		}
	}
	return codexTitleLookup{}, nil
}

var errSQLiteColumnUnavailable = errors.New("sqlite column unavailable")

type codexTitleDatabaseProbe struct {
	FirstThreadID string
	Columns       []string
}

func probeCodexTitleDatabase(path string) (codexTitleDatabaseProbe, error) {
	api, err := loadCodexSQLiteAPI()
	if err != nil {
		return codexTitleDatabaseProbe{}, err
	}
	var lastErr error
	for attempt := 0; attempt <= codexTitleLockRetries; attempt++ {
		probe, probeErr := api.probe(path)
		if probeErr == nil {
			return probe, nil
		}
		var sqliteErr *codexSQLiteError
		if !errors.As(probeErr, &sqliteErr) || !sqliteErr.retryable() || attempt == codexTitleLockRetries {
			return codexTitleDatabaseProbe{}, probeErr
		}
		lastErr = probeErr
		time.Sleep(codexTitleRetryDelay)
	}
	return codexTitleDatabaseProbe{}, lastErr
}

func (a *codexSQLiteAPI) probe(path string) (codexTitleDatabaseProbe, error) {
	db, err := a.openReadOnly(path)
	if err != nil {
		return codexTitleDatabaseProbe{}, err
	}
	defer a.close.Call(db)

	statement, err := a.prepare(db, `SELECT id FROM threads LIMIT 1`)
	if err != nil {
		return codexTitleDatabaseProbe{}, err
	}
	defer a.finalize.Call(statement)

	probe := codexTitleDatabaseProbe{}
	stepCode, _, _ := a.step.Call(statement)
	switch int(stepCode) {
	case sqliteRow:
		probe.FirstThreadID = a.columnString(statement, 0)
	case sqliteDone:
	default:
		return codexTitleDatabaseProbe{}, a.sqliteError(db, "probe_threads", stepCode)
	}

	for _, column := range []string{"name", "title", "first_user_message"} {
		available, availabilityErr := a.columnAvailable(db, column)
		if availabilityErr != nil {
			return codexTitleDatabaseProbe{}, availabilityErr
		}
		if available {
			probe.Columns = append(probe.Columns, column)
		}
	}
	return probe, nil
}

func (a *codexSQLiteAPI) columnAvailable(db uintptr, column string) (bool, error) {
	statement, err := a.prepare(
		db,
		fmt.Sprintf("SELECT %s FROM threads LIMIT 0", column),
	)
	if err != nil {
		if sqliteCodeOf(err) == sqliteError {
			return false, nil
		}
		return false, err
	}
	a.finalize.Call(statement)
	return true, nil
}

func (a *codexSQLiteAPI) queryColumn(db uintptr, column, threadID string) (string, error) {
	statement, err := a.prepare(
		db,
		fmt.Sprintf("SELECT %s FROM threads WHERE id = ? LIMIT 1", column),
	)
	if err != nil {
		if sqliteCodeOf(err) == sqliteError {
			return "", fmt.Errorf("%w: %s", errSQLiteColumnUnavailable, err)
		}
		return "", err
	}
	defer a.finalize.Call(statement)

	threadBytes := append([]byte(threadID), 0)
	code, _, _ := a.bindText.Call(
		statement,
		1,
		uintptr(unsafe.Pointer(&threadBytes[0])),
		sqliteParamEmpty,
		^uintptr(0),
	)
	if int(code) != sqliteOK {
		return "", a.sqliteError(db, "bind_thread_id", uintptr(code))
	}

	stepCode, _, _ := a.step.Call(statement)
	switch int(stepCode) {
	case sqliteRow:
		value := a.columnString(statement, 0)
		return cleanCodexTitle(value), nil
	case sqliteDone:
		return "", nil
	default:
		return "", a.sqliteError(db, "step_title_query", stepCode)
	}
}

func (a *codexSQLiteAPI) openReadOnly(path string) (uintptr, error) {
	var db uintptr
	pathBytes := append([]byte(path), 0)
	code, _, _ := a.openV2.Call(
		uintptr(unsafe.Pointer(&pathBytes[0])),
		uintptr(unsafe.Pointer(&db)),
		sqliteOpenRead,
		0,
	)
	if int(code) != sqliteOK {
		detail := a.errorMessage(db)
		if db != 0 {
			a.close.Call(db)
		}
		return 0, &codexSQLiteError{
			Stage: "open_database",
			Code:  int(code),
			Err:   errors.New(detail),
		}
	}
	a.busyTimeout.Call(db, uintptr(codexTitleBusyTimeout.Milliseconds()))
	return db, nil
}

func (a *codexSQLiteAPI) prepare(db uintptr, query string) (uintptr, error) {
	var statement uintptr
	queryBytes := append([]byte(query), 0)
	code, _, _ := a.prepareV2.Call(
		db,
		uintptr(unsafe.Pointer(&queryBytes[0])),
		sqliteParamEmpty,
		uintptr(unsafe.Pointer(&statement)),
		0,
	)
	if int(code) != sqliteOK || statement == 0 {
		return 0, a.sqliteError(db, "prepare_title_query", uintptr(code))
	}
	return statement, nil
}

func (a *codexSQLiteAPI) sqliteError(db uintptr, stage string, code uintptr) error {
	return &codexSQLiteError{
		Stage: stage,
		Code:  int(code),
		Err:   errors.New(a.errorMessage(db)),
	}
}

func (a *codexSQLiteAPI) errorMessage(db uintptr) string {
	if db == 0 {
		return "sqlite database handle is unavailable"
	}
	pointer, _, _ := a.errMsg.Call(db)
	return cString(pointer)
}

func (a *codexSQLiteAPI) columnString(statement uintptr, index int) string {
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

func sqliteCodeOf(err error) int {
	var sqliteErr *codexSQLiteError
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
	for len(data) < codexTitleErrorLimit {
		move.Call(uintptr(unsafe.Pointer(&scratch[0])), pointer+uintptr(len(data)), 1)
		if scratch[0] == 0 {
			return string(data)
		}
		data = append(data, scratch[0])
	}
	return "sqlite error message exceeds diagnostic limit"
}

// loadWindowsMoveMemory resolves a native memory copy helper. Pointers returned
// by winsqlite3.dll are copied into Go memory with it instead of being converted
// back into an unsafe.Pointer, keeping unsafe usage limited to Go-owned buffers.
func loadWindowsMoveMemory() (*syscall.LazyProc, error) {
	winMoveMemoryOnce.Do(func() {
		for _, library := range []string{"kernel32.dll", "ntdll.dll"} {
			procedure := syscall.NewLazyDLL(library).NewProc("RtlMoveMemory")
			if err := procedure.Find(); err != nil {
				winMoveMemoryErr = fmt.Errorf("%s export RtlMoveMemory: %w", library, err)
				continue
			}
			winMoveMemoryProc = procedure
			winMoveMemoryErr = nil
			return
		}
	})
	return winMoveMemoryProc, winMoveMemoryErr
}
