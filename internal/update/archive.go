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

const maxExtractedBytes = int64(200 << 20)

func extractZip(archivePath, destination string) error {
	archive, err := zip.OpenReader(archivePath)
	if err != nil {
		return fmt.Errorf("打开更新包失败：%w", err)
	}
	defer archive.Close()

	var extracted int64
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
		extracted += int64(entry.UncompressedSize64)
		if extracted > maxExtractedBytes {
			return fmt.Errorf("解压后的更新内容超过允许大小 %d 字节", maxExtractedBytes)
		}
		if err := extractZipFile(entry, target); err != nil {
			return err
		}
	}
	return nil
}

func extractZipFile(entry *zip.File, target string) error {
	if err := os.MkdirAll(filepath.Dir(target), 0700); err != nil {
		return fmt.Errorf("创建更新目录失败：%w", err)
	}
	source, err := entry.Open()
	if err != nil {
		return fmt.Errorf("读取更新包文件失败：%w", err)
	}
	defer source.Close()

	mode := os.FileMode(0600)
	if strings.EqualFold(filepath.Ext(target), ".exe") || strings.EqualFold(filepath.Ext(target), ".ps1") {
		mode = 0700
	}
	destination, err := os.OpenFile(target, os.O_CREATE|os.O_TRUNC|os.O_WRONLY, mode)
	if err != nil {
		return fmt.Errorf("写入更新文件失败：%w", err)
	}
	if _, err := io.Copy(destination, source); err != nil {
		_ = destination.Close()
		return fmt.Errorf("解压更新文件失败：%w", err)
	}
	if err := destination.Close(); err != nil {
		return fmt.Errorf("关闭更新文件失败：%w", err)
	}
	return nil
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
