package reply

import (
	"bufio"
	"bytes"
	"encoding/json"
	"fmt"
	"io"
	"os"
	"path/filepath"
)

const (
	privateDirPerm  os.FileMode = 0700
	privateFilePerm os.FileMode = 0600

	initialJSONLScanBuffer = 64 * 1024
	maxJSONLLineBytes      = 2 * 1024 * 1024

	defaultJSONLCompactMinBytes          int64 = 2 * 1024 * 1024
	defaultJSONLCompactMinReclaimedBytes int64 = 512 * 1024
	defaultJSONLCompactMinReclaimedRatio       = 0.25
)

type jsonlCompactionOptions struct {
	MinBytes          int64
	MinReclaimedBytes int64
	MinReclaimedRatio float64
}

var defaultJSONLCompactionOptions = jsonlCompactionOptions{
	MinBytes:          defaultJSONLCompactMinBytes,
	MinReclaimedBytes: defaultJSONLCompactMinReclaimedBytes,
	MinReclaimedRatio: defaultJSONLCompactMinReclaimedRatio,
}

func newJSONLScanner(reader io.Reader) *bufio.Scanner {
	scanner := bufio.NewScanner(reader)
	scanner.Buffer(make([]byte, initialJSONLScanBuffer), maxJSONLLineBytes)
	return scanner
}

func appendJSONLine(path string, value any) error {
	data, err := marshalJSONLine(value)
	if err != nil {
		return err
	}
	if err := ensureStateDir(path); err != nil {
		return err
	}
	return withFileLock(path+".lock", func() error {
		return appendJSONData(path, data)
	})
}

func appendJSONLineFiltered(path string, value any, keep func([]byte) bool) error {
	data, err := marshalJSONLine(value)
	if err != nil {
		return err
	}
	if err := ensureStateDir(path); err != nil {
		return err
	}
	return withFileLock(path+".lock", func() error {
		if err := appendJSONData(path, data); err != nil {
			return err
		}
		// Compaction is physical maintenance; a failure must not turn a
		// durable append into an apparent logical failure.
		_ = compactJSONL(path, keep, defaultJSONLCompactionOptions)
		return nil
	})
}

// compactJSONL rewrites a JSONL file when enough stale or corrupt lines
// accumulated. Callers must hold the corresponding .lock file.
func compactJSONL(path string, keep func([]byte) bool, options jsonlCompactionOptions) error {
	if keep == nil {
		return nil
	}
	info, err := os.Stat(path)
	if err != nil {
		if os.IsNotExist(err) {
			return nil
		}
		return err
	}
	if options.MinBytes > 0 && info.Size() < options.MinBytes {
		return nil
	}

	file, err := os.Open(path)
	if err != nil {
		return err
	}

	temp, err := os.CreateTemp(filepath.Dir(path), filepath.Base(path)+".compact-*")
	if err != nil {
		_ = file.Close()
		return err
	}
	tempPath := temp.Name()
	_ = temp.Chmod(privateFilePerm)
	removeTemp := func() {
		_ = file.Close()
		_ = temp.Close()
		_ = os.Remove(tempPath)
	}

	var removedBytes int64
	scanner := newJSONLScanner(file)
	for scanner.Scan() {
		raw := scanner.Bytes()
		line := bytes.TrimSpace(raw)
		lineBytes := int64(len(raw) + 1)
		if len(line) == 0 || !keep(line) {
			removedBytes += lineBytes
			continue
		}
		if _, err := temp.Write(line); err != nil {
			removeTemp()
			return err
		}
		if _, err := temp.Write([]byte{'\n'}); err != nil {
			removeTemp()
			return err
		}
	}
	if err := scanner.Err(); err != nil {
		removeTemp()
		return err
	}
	if err := file.Close(); err != nil {
		removeTemp()
		return err
	}
	if err := temp.Sync(); err != nil {
		removeTemp()
		return err
	}
	if err := temp.Close(); err != nil {
		_ = os.Remove(tempPath)
		return err
	}

	if removedBytes < options.MinReclaimedBytes ||
		float64(removedBytes) < float64(info.Size())*options.MinReclaimedRatio {
		_ = os.Remove(tempPath)
		return nil
	}
	if err := os.Rename(tempPath, path); err != nil {
		_ = os.Remove(tempPath)
		return err
	}
	return nil
}

func appendJSONLineUnlocked(path string, value any) error {
	data, err := marshalJSONLine(value)
	if err != nil {
		return err
	}
	if err := ensureStateDir(path); err != nil {
		return err
	}
	return appendJSONData(path, data)
}

func marshalJSONLine(value any) ([]byte, error) {
	data, err := json.Marshal(value)
	if err != nil {
		return nil, fmt.Errorf("reply: encode state: %w", err)
	}
	return data, nil
}

func ensureStateDir(path string) error {
	if err := os.MkdirAll(filepath.Dir(path), privateDirPerm); err != nil {
		return fmt.Errorf("reply: create state directory: %w", err)
	}
	return nil
}

func appendJSONData(path string, data []byte) error {
	file, err := os.OpenFile(path, os.O_APPEND|os.O_CREATE|os.O_WRONLY, privateFilePerm)
	if err != nil {
		return fmt.Errorf("reply: open state: %w", err)
	}
	defer file.Close()
	_ = file.Chmod(privateFilePerm)
	if _, err := file.Write(append(data, '\n')); err != nil {
		return fmt.Errorf("reply: write state: %w", err)
	}
	if err := file.Sync(); err != nil {
		return fmt.Errorf("reply: sync state: %w", err)
	}
	return nil
}
