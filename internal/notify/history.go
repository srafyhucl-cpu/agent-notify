package notify

import (
	"bufio"
	"bytes"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"os"
	"path/filepath"
	"strings"
	"time"

	"github.com/srafyhucl-cpu/agent-notify/internal/config"
)

const (
	// 历史记录按 JSON Lines 存储；读取与裁剪均使用 bufio.NewReaderSize 按块读取
	// （见 trimHistoryTo），不再依赖固定大小的行缓冲区常量。
	defaultHistoryLimit = 50

	// 倒序扫描时每次从文件末尾读取的块大小。
	historyReadChunkBytes = 64 * 1024
	// 日志超过该大小后裁剪旧记录，避免无限增长拖慢磁盘与内存。
	historyMaxBytes = 4 * 1024 * 1024
	// 裁剪后保留的字节数（从文件末尾截取，按整行保留）。
	historyKeepBytes = 2 * 1024 * 1024
)

// HistoryItem is one structured push record. The file uses JSON Lines so
// summaries can contain newlines without corrupting the history.
type HistoryItem struct {
	Timestamp string `json:"timestamp"`
	Agent     string `json:"agent,omitempty"`
	Session   string `json:"session,omitempty"`
	Title     string `json:"title"`
	Summary   string `json:"summary,omitempty"`
	Status    string `json:"status"`
	Error     string `json:"error,omitempty"`
	MessageID string `json:"messageID,omitempty"`
	ClientID  string `json:"clientID,omitempty"`
}

func (h HistoryItem) LocalTime() time.Time {
	if value, err := time.Parse(time.RFC3339Nano, h.Timestamp); err == nil {
		return value.Local()
	}
	return time.Time{}
}

func appendHistory(item HistoryItem, logPath string) error {
	if strings.TrimSpace(logPath) == "" {
		logPath = config.GetPaths().PushLog
	}
	if strings.TrimSpace(item.Timestamp) == "" {
		item.Timestamp = time.Now().Format(time.RFC3339Nano)
	}
	if err := os.MkdirAll(filepath.Dir(logPath), 0700); err != nil {
		return fmt.Errorf("create history directory: %w", err)
	}
	// 先按大小裁剪再追加，避免日志无限增长；裁剪失败不阻塞本次写入。
	if err := trimHistory(logPath); err != nil {
		return fmt.Errorf("trim history: %w", err)
	}
	data, err := json.Marshal(item)
	if err != nil {
		return fmt.Errorf("encode history: %w", err)
	}
	file, err := os.OpenFile(logPath, os.O_APPEND|os.O_CREATE|os.O_WRONLY, 0600)
	if err != nil {
		return fmt.Errorf("open history: %w", err)
	}
	defer file.Close()
	if _, err := file.Write(append(data, '\n')); err != nil {
		return fmt.Errorf("write history: %w", err)
	}
	return nil
}

// trimHistory 在日志超过 historyMaxBytes 时只保留末尾 historyKeepBytes 的整行记录。
// 采用"临时文件 + 原子替换"：并发读取只会看到旧文件或新文件；极端情况下
// 与其它写进程的毫秒级竞争可能丢一条记录，但避免了无限增长。
func trimHistory(logPath string) error {
	return trimHistoryTo(logPath, historyMaxBytes, historyKeepBytes)
}

func trimHistoryTo(logPath string, maxBytes, keepBytes int64) error {
	info, err := os.Stat(logPath)
	if err != nil {
		if os.IsNotExist(err) {
			return nil
		}
		return err
	}
	if info.Size() <= maxBytes {
		return nil
	}

	file, err := os.Open(logPath)
	if err != nil {
		return err
	}
	if _, err := file.Seek(info.Size()-keepBytes, io.SeekStart); err != nil {
		file.Close()
		return err
	}
	reader := bufio.NewReaderSize(file, historyReadChunkBytes)
	// 丢弃截断处的残行，保证保留内容都是完整记录。
	if _, err := reader.ReadBytes('\n'); err != nil && !errors.Is(err, io.EOF) {
		file.Close()
		return err
	}
	tail, err := io.ReadAll(io.LimitReader(reader, keepBytes))
	// Windows 下必须先关闭再重命名，否则报 Access is denied。
	if closeErr := file.Close(); err == nil {
		err = closeErr
	}
	if err != nil {
		return err
	}
	return writeHistoryAtomic(logPath, tail, 0600)
}

func writeHistoryAtomic(logPath string, data []byte, perm os.FileMode) error {
	dir := filepath.Dir(logPath)
	temp, err := os.CreateTemp(dir, ".push-*.tmp")
	if err != nil {
		return err
	}
	tempPath := temp.Name()
	defer os.Remove(tempPath)
	if _, err := temp.Write(data); err != nil {
		temp.Close()
		return err
	}
	if err := temp.Chmod(perm); err != nil {
		temp.Close()
		return err
	}
	if err := temp.Close(); err != nil {
		return err
	}
	return os.Rename(tempPath, logPath)
}

// GetHistory 返回最近的 limit 条记录（新的在前）。实现从文件末尾倒序扫描：
// 历史再长也只读取并解析末尾若干块，不再全量解析整个日志。
func GetHistory(limit int, logPath string) ([]HistoryItem, error) {
	if strings.TrimSpace(logPath) == "" {
		logPath = config.GetPaths().PushLog
	}
	if limit <= 0 {
		limit = defaultHistoryLimit
	}

	file, err := os.Open(logPath)
	if err != nil {
		if os.IsNotExist(err) {
			return nil, nil
		}
		return nil, err
	}
	defer file.Close()

	size, err := file.Seek(0, io.SeekEnd)
	if err != nil {
		return nil, err
	}
	if size == 0 {
		return nil, nil
	}

	items := make([]HistoryItem, 0, limit)
	offset := size
	leftover := make([]byte, 0, historyReadChunkBytes)
	for offset > 0 && len(items) < limit {
		readSize := int64(historyReadChunkBytes)
		if offset < readSize {
			readSize = offset
		}
		offset -= readSize
		chunk := make([]byte, readSize)
		if _, err := file.ReadAt(chunk, offset); err != nil {
			return nil, err
		}
		data := append(chunk, leftover...)

		for len(items) < limit {
			index := bytes.LastIndexByte(data, '\n')
			if index < 0 {
				break
			}
			line := data[index+1:]
			data = data[:index]
			if item, ok := parseHistoryLine(line); ok {
				items = append(items, item)
			}
		}
		leftover = append(leftover[:0], data...)
	}

	// 文件开头可能没有换行结尾，最后一段要单独解析。
	if len(items) < limit {
		if item, ok := parseHistoryLine(leftover); ok {
			items = append(items, item)
		}
	}
	return items, nil
}

func parseHistoryLine(line []byte) (HistoryItem, bool) {
	trimmed := bytes.TrimSpace(line)
	if len(trimmed) == 0 {
		return HistoryItem{}, false
	}
	var item HistoryItem
	if err := json.Unmarshal(trimmed, &item); err != nil {
		return HistoryItem{}, false
	}
	return item, true
}

// ClearHistory removes the push log file.
func ClearHistory(logPath string) error {
	if strings.TrimSpace(logPath) == "" {
		logPath = config.GetPaths().PushLog
	}
	err := os.Remove(logPath)
	if err != nil && !os.IsNotExist(err) {
		return err
	}
	return nil
}
