//go:build windows

package update

import (
	"bufio"
	"crypto/sha256"
	"encoding/hex"
	"fmt"
	"io"
	"os"
	"strings"
)

func verifyChecksum(checksumsPath, archiveName, archivePath string) error {
	expected, err := readChecksum(checksumsPath, archiveName)
	if err != nil {
		return err
	}
	file, err := os.Open(archivePath)
	if err != nil {
		return fmt.Errorf("读取更新包失败：%w", err)
	}
	defer file.Close()
	hash := sha256.New()
	if _, err := io.Copy(hash, file); err != nil {
		return fmt.Errorf("计算更新包校验值失败：%w", err)
	}
	actual := hex.EncodeToString(hash.Sum(nil))
	if !strings.EqualFold(expected, actual) {
		return fmt.Errorf("更新包 SHA256 校验失败：期望 %s，实际 %s", expected, actual)
	}
	return nil
}

func readChecksum(checksumsPath, archiveName string) (string, error) {
	file, err := os.Open(checksumsPath)
	if err != nil {
		return "", fmt.Errorf("读取 %s 失败：%w", checksumName, err)
	}
	defer file.Close()

	scanner := bufio.NewScanner(file)
	for scanner.Scan() {
		fields := strings.Fields(scanner.Text())
		if len(fields) < 2 {
			continue
		}
		name := strings.TrimPrefix(fields[len(fields)-1], "*")
		if name != archiveName {
			continue
		}
		checksum := strings.ToLower(strings.TrimSpace(fields[0]))
		if len(checksum) != sha256.Size*2 {
			return "", fmt.Errorf("更新包校验值长度无效：%q", checksum)
		}
		if _, err := hex.DecodeString(checksum); err != nil {
			return "", fmt.Errorf("更新包校验值格式无效：%w", err)
		}
		return checksum, nil
	}
	if err := scanner.Err(); err != nil {
		return "", fmt.Errorf("读取 %s 失败：%w", checksumName, err)
	}
	return "", fmt.Errorf("%s 未包含 %s", checksumName, archiveName)
}
