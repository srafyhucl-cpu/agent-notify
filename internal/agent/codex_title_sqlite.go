package agent

import (
	"errors"
	"fmt"

	"github.com/srafyhucl-cpu/agent-notify/internal/winsqlite"
)

const (
	sqliteOK             = winsqlite.CodeOK
	sqliteError          = winsqlite.CodeError
	sqliteBusy           = winsqlite.CodeBusy
	codexTitleRetryDelay = winsqlite.RetryDelay
)

type codexSQLiteError = winsqlite.Error

type codexTitleLookup struct {
	Name    string
	Source  string
	Retries int
}

var errSQLiteColumnUnavailable = errors.New("sqlite column unavailable")

func queryCodexTitleDatabase(path, threadID string) (codexTitleLookup, error) {
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
		row, queryErr := winsqlite.ReadRow(
			path,
			"prepare_title_query",
			1,
			fmt.Sprintf("SELECT %s FROM threads WHERE id = ? LIMIT 1", column.name),
			threadID,
		)
		if queryErr != nil {
			if winsqlite.CodeOf(queryErr) == sqliteError {
				lastErr = fmt.Errorf("%w: %v", errSQLiteColumnUnavailable, queryErr)
				continue
			}
			return codexTitleLookup{}, queryErr
		}
		queryable = true
		if !row.Found {
			continue
		}
		if name := cleanCodexTitle(row.Values[0]); name != "" {
			return codexTitleLookup{Name: name, Source: column.source, Retries: row.Retries}, nil
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

type codexTitleDatabaseProbe struct {
	FirstThreadID string
	Columns       []string
}

func probeCodexTitleDatabase(path string) (codexTitleDatabaseProbe, error) {
	row, err := winsqlite.ReadRow(path, "probe_threads", 1, "SELECT id FROM threads LIMIT 1")
	if err != nil {
		return codexTitleDatabaseProbe{}, err
	}

	probe := codexTitleDatabaseProbe{}
	if row.Found {
		probe.FirstThreadID = row.Values[0]
	}
	for _, column := range []string{"name", "title", "first_user_message"} {
		_, availabilityErr := winsqlite.ReadRow(
			path,
			"probe_title_columns",
			1,
			fmt.Sprintf("SELECT %s FROM threads LIMIT 0", column),
		)
		if availabilityErr != nil {
			if winsqlite.CodeOf(availabilityErr) == sqliteError {
				continue
			}
			return codexTitleDatabaseProbe{}, availabilityErr
		}
		probe.Columns = append(probe.Columns, column)
	}
	return probe, nil
}
