package agent

import (
	"fmt"
	"path/filepath"
	"strings"
)

const (
	CodexTitleStatusNormal   = "正常"
	CodexTitleStatusDegraded = "降级"
	CodexTitleStatusFailed   = "故障"
)

// CodexTitleHealth summarizes the read-only Codex session title dependency.
type CodexTitleHealth struct {
	Status string
	Detail string
}

// CheckCodexTitleHealth performs a real read-only SQLite title probe.
func CheckCodexTitleHealth() CodexTitleHealth {
	if _, err := loadCodexSQLiteAPI(); err != nil {
		return CodexTitleHealth{
			Status: CodexTitleStatusFailed,
			Detail: "winsqlite3.dll 或导出函数不可用: " + err.Error(),
		}
	}

	resolver := windowsCodexTitleResolver{}
	home := resolver.codexHome()
	databases, err := discoverCodexStateDatabases(home)
	if err != nil {
		return CodexTitleHealth{
			Status: CodexTitleStatusFailed,
			Detail: "未找到可读取的 Codex 状态数据库: " + err.Error(),
		}
	}

	var selectedDatabase string
	var probe codexTitleDatabaseProbe
	var lastErr error
	for _, database := range databases {
		candidate, probeErr := probeCodexTitleDatabase(database)
		if probeErr != nil {
			lastErr = probeErr
			continue
		}
		if len(candidate.Columns) == 0 {
			lastErr = fmt.Errorf("%s: threads table has no compatible title column", filepath.Base(database))
			continue
		}
		if selectedDatabase == "" || candidate.FirstThreadID != "" {
			selectedDatabase = database
			probe = candidate
		}
		if candidate.FirstThreadID != "" {
			break
		}
	}
	if selectedDatabase == "" {
		detail := "状态数据库只读查询失败"
		if lastErr != nil {
			detail += ": " + lastErr.Error()
		}
		return CodexTitleHealth{Status: CodexTitleStatusFailed, Detail: detail}
	}

	databaseName := filepath.Base(selectedDatabase)
	if probe.FirstThreadID == "" {
		return CodexTitleHealth{
			Status: CodexTitleStatusDegraded,
			Detail: fmt.Sprintf("%s 可读，但 threads 表暂无线程记录", databaseName),
		}
	}
	lookup, err := queryCodexTitleDatabase(selectedDatabase, probe.FirstThreadID)
	if err != nil {
		return CodexTitleHealth{
			Status: CodexTitleStatusFailed,
			Detail: "真实标题查询失败: " + err.Error(),
		}
	}
	if lookup.Name == "" {
		return CodexTitleHealth{
			Status: CodexTitleStatusDegraded,
			Detail: "threads 表可查询，但当前线程名称为空",
		}
	}

	status := CodexTitleStatusNormal
	prefix := ""
	if len(probe.Columns) < 3 {
		status = CodexTitleStatusDegraded
		prefix = "部分字段缺失，使用 " + strings.Join(probe.Columns, ",") + " 降级读取；"
	}
	return CodexTitleHealth{
		Status: status,
		Detail: prefix + fmt.Sprintf(
			"%s 只读解析成功（来源 %s）",
			databaseName,
			lookup.Source,
		),
	}
}
