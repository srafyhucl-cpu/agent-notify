package agent

import (
	"bufio"
	"encoding/json"
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"sort"
	"strconv"
	"strings"
	"time"
)

const (
	codexTitleSourceName           = "threads.name"
	codexTitleSourceTitle          = "threads.title"
	codexTitleSourceFirstMessage   = "threads.first_user_message"
	codexTitleSourceSessionIndex   = "session_index.jsonl"
	codexTitleSourcePayload        = "codex_payload"
	codexTitleSourceFallback       = "fallback"
	codexTitleSourceUnavailable    = "unavailable"
	codexStateDatabasePrefix       = "state_"
	codexStateDatabaseSuffix       = ".sqlite"
	codexSessionIndexFileName      = "session_index.jsonl"
	codexTitleSessionIndexLineSize = 1024 * 1024
)

type codexTitleResolution struct {
	Name           string
	Source         string
	Warning        string
	FailureStage   string
	FailureDetail  string
	FallbackSource string
	SQLiteCode     int
	Retries        int
}

type codexTitleResolver interface {
	Resolve(threadID, payloadTitle string) codexTitleResolution
}

type windowsCodexTitleResolver struct {
	home string
}

func (r windowsCodexTitleResolver) Resolve(threadID, payloadTitle string) codexTitleResolution {
	return resolveCodexTitle(threadID, payloadTitle, r.codexHome())
}

func (r windowsCodexTitleResolver) codexHome() string {
	if configured := strings.TrimSpace(r.home); configured != "" {
		return configured
	}
	if configured := strings.TrimSpace(os.Getenv("CODEX_HOME")); configured != "" {
		return configured
	}
	if home := strings.TrimSpace(os.Getenv("USERPROFILE")); home != "" {
		return filepath.Join(home, ".codex")
	}
	if home, err := os.UserHomeDir(); err == nil {
		return filepath.Join(home, ".codex")
	}
	return ""
}

func resolveCodexTitle(threadID, payloadTitle, codexHome string) codexTitleResolution {
	payloadTitle = cleanCodexTitle(payloadTitle)
	if strings.TrimSpace(threadID) == "" {
		return payloadTitleResolution(payloadTitle, "", "", 0)
	}

	databases, discoverErr := discoverCodexStateDatabases(codexHome)
	if discoverErr != nil {
		return localTitleFailure(
			payloadTitle,
			codexTitleSourceUnavailable,
			"discover_database",
			discoverErr.Error(),
			0,
			0,
			"标题读取失败：未找到 Codex 会话数据库，已回退为任务摘要。",
		)
	}

	readSucceeded := false
	var lastFailure *codexSQLiteError
	for _, database := range databases {
		lookup, err := queryCodexTitleDatabase(database, threadID)
		if err != nil {
			var sqliteErr *codexSQLiteError
			if errors.As(err, &sqliteErr) {
				lastFailure = sqliteErr
			} else {
				lastFailure = &codexSQLiteError{Stage: "query", Err: err}
			}
			continue
		}
		readSucceeded = true
		if lookup.Name != "" {
			return codexTitleResolution{
				Name:       lookup.Name,
				Source:     lookup.Source,
				Retries:    lookup.Retries,
				SQLiteCode: sqliteOK,
			}
		}
	}

	if !readSucceeded {
		stage := "open_database"
		detail := "no queryable Codex state database"
		code := 0
		retries := 0
		if lastFailure != nil {
			stage = lastFailure.Stage
			detail = lastFailure.Error()
			code = lastFailure.Code
			retries = lastFailure.Retries
		}
		return localTitleFailure(
			payloadTitle,
			codexTitleSourceUnavailable,
			stage,
			detail,
			code,
			retries,
			"标题读取失败：无法读取 Codex 会话数据库，已回退为任务摘要。",
		)
	}

	if name, err := lookupSessionIndexName(codexHome, threadID); err == nil && name != "" {
		return codexTitleResolution{
			Name:           name,
			Source:         codexTitleSourceSessionIndex,
			FallbackSource: codexTitleSourceSessionIndex,
		}
	}
	return payloadTitleResolution(payloadTitle, "", "", 0)
}

func payloadTitleResolution(payloadTitle, failureStage, failureDetail string, sqliteCode int) codexTitleResolution {
	if name := cleanCodexTitle(payloadTitle); name != "" {
		return codexTitleResolution{
			Name:           name,
			Source:         codexTitleSourcePayload,
			FailureStage:   failureStage,
			FailureDetail:  failureDetail,
			FallbackSource: codexTitleSourcePayload,
			SQLiteCode:     sqliteCode,
		}
	}
	return codexTitleResolution{
		Name:           "跑完了",
		Source:         codexTitleSourceFallback,
		FailureStage:   failureStage,
		FailureDetail:  failureDetail,
		FallbackSource: codexTitleSourceFallback,
		SQLiteCode:     sqliteCode,
	}
}

func localTitleFailure(
	payloadTitle, source, stage, detail string,
	sqliteCode, retries int,
	warning string,
) codexTitleResolution {
	resolution := payloadTitleResolution(payloadTitle, stage, detail, sqliteCode)
	resolution.FallbackSource = resolution.Source
	resolution.Source = source
	resolution.Warning = warning
	resolution.Retries = retries
	return resolution
}

func discoverCodexStateDatabases(codexHome string) ([]string, error) {
	codexHome = strings.TrimSpace(codexHome)
	if codexHome == "" {
		return nil, errors.New("CODEX_HOME is empty")
	}
	entries, err := os.ReadDir(codexHome)
	if err != nil {
		return nil, fmt.Errorf("read Codex home %s: %w", codexHome, err)
	}

	type candidate struct {
		path     string
		version  int
		hasOrder bool
		modTime  time.Time
	}
	var candidates []candidate
	for _, entry := range entries {
		if entry.IsDir() {
			continue
		}
		name := entry.Name()
		if !strings.HasPrefix(name, codexStateDatabasePrefix) ||
			!strings.HasSuffix(name, codexStateDatabaseSuffix) {
			continue
		}
		versionText := strings.TrimSuffix(
			strings.TrimPrefix(name, codexStateDatabasePrefix),
			codexStateDatabaseSuffix,
		)
		version, parseErr := strconv.Atoi(versionText)
		info, statErr := entry.Info()
		if statErr != nil {
			continue
		}
		candidates = append(candidates, candidate{
			path:     filepath.Join(codexHome, name),
			version:  version,
			hasOrder: parseErr == nil,
			modTime:  info.ModTime(),
		})
	}
	if len(candidates) == 0 {
		return nil, fmt.Errorf("no %s*%s in %s", codexStateDatabasePrefix, codexStateDatabaseSuffix, codexHome)
	}
	sort.Slice(candidates, func(i, j int) bool {
		left, right := candidates[i], candidates[j]
		if left.hasOrder != right.hasOrder {
			return left.hasOrder
		}
		if left.hasOrder && left.version != right.version {
			return left.version > right.version
		}
		if !left.modTime.Equal(right.modTime) {
			return left.modTime.After(right.modTime)
		}
		return left.path < right.path
	})

	paths := make([]string, 0, len(candidates))
	for _, candidate := range candidates {
		paths = append(paths, candidate.path)
	}
	return paths, nil
}

func lookupSessionIndexName(codexHome, threadID string) (string, error) {
	path := filepath.Join(codexHome, codexSessionIndexFileName)
	file, err := os.Open(path)
	if err != nil {
		if os.IsNotExist(err) {
			return "", nil
		}
		return "", err
	}
	defer file.Close()

	scanner := bufio.NewScanner(file)
	scanner.Buffer(make([]byte, 64*1024), codexTitleSessionIndexLineSize)
	name := ""
	for scanner.Scan() {
		var item struct {
			ID         string `json:"id"`
			ThreadName string `json:"thread_name"`
		}
		if json.Unmarshal(scanner.Bytes(), &item) != nil || item.ID != threadID {
			continue
		}
		if candidate := cleanCodexTitle(item.ThreadName); candidate != "" {
			name = candidate
		}
	}
	if err := scanner.Err(); err != nil {
		return "", err
	}
	return name, nil
}

func cleanCodexTitle(value string) string {
	return strings.Join(strings.Fields(value), " ")
}
