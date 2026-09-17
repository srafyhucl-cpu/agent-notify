// Package diag 提供诊断日志的最小追加写能力。
// 诊断日志只用于排查问题，任何错误都不允许影响主流程。
package diag

import (
	"os"
	"path/filepath"
)

// Append 以追加方式把 line 写入 path；目录不存在时创建，权限取 0700/0600。
// 任何错误都被忽略：诊断日志不允许影响主流程。
func Append(path, line string) {
	dir := filepath.Dir(path)
	if dir != "" {
		_ = os.MkdirAll(dir, 0700)
	}
	file, err := os.OpenFile(path, os.O_APPEND|os.O_CREATE|os.O_WRONLY, 0600)
	if err != nil {
		return
	}
	defer file.Close()
	_, _ = file.WriteString(line)
}
