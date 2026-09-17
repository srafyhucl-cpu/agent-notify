package diag

import (
	"os"
	"path/filepath"
	"testing"
)

func TestAppendCreatesDirectoryAndAppends(t *testing.T) {
	path := filepath.Join(t.TempDir(), "nested", "debug.log")

	Append(path, "first\n")
	Append(path, "second\n")

	data, err := os.ReadFile(path)
	if err != nil {
		t.Fatalf("ReadFile: %v", err)
	}
	if string(data) != "first\nsecond\n" {
		t.Fatalf("content = %q, want appended lines", data)
	}
}

// Append 的契约是「诊断日志绝不影响主流程」：写入失败必须静默返回，而不是 panic 或返回错误。
func TestAppendIgnoresWriteFailure(t *testing.T) {
	// 以一个目录作为目标路径，OpenFile 必然失败。
	dir := t.TempDir()
	Append(dir, "ignored")
}
