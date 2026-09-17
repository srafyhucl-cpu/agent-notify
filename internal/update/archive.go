//go:build windows

package update

import (
	"archive/zip"
	"errors"
	"fmt"
	"io"
	"os"
	"path"
	"path/filepath"
	"strings"
)

const maxExtractedBytes = uint64(200 << 20)

// withinExtractionBudget 判断 declared 字节是否还在剩余预算内。
// 全程使用 uint64 累计，避免声明大小溢出为负数绕过检查。
func withinExtractionBudget(declared, used, max uint64) bool {
	if used > max {
		return false
	}
	return declared <= max-used
}

func extractZip(archivePath, destination string) error {
	archive, err := zip.OpenReader(archivePath)
	if err != nil {
		return fmt.Errorf("打开更新包失败：%w", err)
	}
	defer archive.Close()

	var extracted uint64
	for _, entry := range archive.File {
		name := path.Clean(strings.ReplaceAll(entry.Name, "\\", "/"))
		nativeName := filepath.FromSlash(name)
		if name == "." || path.IsAbs(name) || filepath.IsAbs(nativeName) || filepath.VolumeName(nativeName) != "" ||
			name == ".." || strings.HasPrefix(name, "../") {
			return fmt.Errorf("更新包包含越界路径：%s", entry.Name)
		}
		target := filepath.Join(destination, nativeName)
		if !pathInside(destination, target) {
			return fmt.Errorf("更新包包含越界路径：%s", entry.Name)
		}
		if entry.FileInfo().IsDir() {
			if err := os.MkdirAll(target, 0700); err != nil {
				return fmt.Errorf("创建更新目录失败：%w", err)
			}
			continue
		}
		if entry.Mode()&os.ModeSymlink != 0 {
			return fmt.Errorf("更新包包含不支持的符号链接：%s", entry.Name)
		}
		if !withinExtractionBudget(entry.UncompressedSize64, extracted, maxExtractedBytes) {
			return fmt.Errorf("解压后的更新内容超过允许大小 %d 字节", maxExtractedBytes)
		}
		written, err := extractZipFile(entry, target, maxExtractedBytes-extracted)
		if err != nil {
			return err
		}
		extracted += written
	}
	return nil
}

// extractZipFile 将单个 entry 解压到 target，最多写入 limit 字节；
// 返回实际写入字节数，超过 limit 时返回错误。
func extractZipFile(entry *zip.File, target string, limit uint64) (uint64, error) {
	if err := os.MkdirAll(filepath.Dir(target), 0700); err != nil {
		return 0, fmt.Errorf("创建更新目录失败：%w", err)
	}
	source, err := entry.Open()
	if err != nil {
		return 0, fmt.Errorf("读取更新包文件失败：%w", err)
	}
	defer source.Close()

	mode := os.FileMode(0600)
	if strings.EqualFold(filepath.Ext(target), ".exe") || strings.EqualFold(filepath.Ext(target), ".ps1") {
		mode = 0700
	}
	destination, err := os.OpenFile(target, os.O_CREATE|os.O_TRUNC|os.O_WRONLY, mode)
	if err != nil {
		return 0, fmt.Errorf("写入更新文件失败：%w", err)
	}
	written, err := io.Copy(destination, io.LimitReader(source, int64(limit)+1))
	if err != nil {
		_ = destination.Close()
		return uint64(written), fmt.Errorf("解压更新文件失败：%w", err)
	}
	if uint64(written) > limit {
		_ = destination.Close()
		return uint64(written), fmt.Errorf("解压后的更新内容超过允许大小 %d 字节", maxExtractedBytes)
	}
	if err := destination.Close(); err != nil {
		return uint64(written), fmt.Errorf("关闭更新文件失败：%w", err)
	}
	return uint64(written), nil
}

func validatePreparedRelease(releaseDir, installerPath, executablePath, versionPath, expectedVersion string) error {
	info, err := os.Stat(releaseDir)
	if err != nil || !info.IsDir() {
		return errors.New("更新包缺少 Agent-notify 根目录")
	}
	for _, required := range []string{installerPath, executablePath, versionPath} {
		info, err := os.Stat(required)
		if err != nil || info.IsDir() {
			return fmt.Errorf("更新包缺少文件：%s", required)
		}
	}
	versionBytes, err := os.ReadFile(versionPath)
	if err != nil {
		return fmt.Errorf("读取更新包版本失败：%w", err)
	}
	version, ok := normalizeVersion(string(versionBytes))
	if !ok || version != expectedVersion {
		return fmt.Errorf("更新包版本不一致：期望 %s，实际 %q", expectedVersion, strings.TrimSpace(string(versionBytes)))
	}
	return nil
}

func pathInside(root, target string) bool {
	root = filepath.Clean(root)
	target = filepath.Clean(target)
	relative, err := filepath.Rel(root, target)
	if err != nil {
		return false
	}
	return relative == "." || (relative != ".." && !strings.HasPrefix(relative, ".."+string(os.PathSeparator)))
}
