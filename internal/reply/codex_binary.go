package reply

import (
	"errors"
	"os"
	"os/exec"
	"path/filepath"
	"sort"
	"strings"
	"time"
)

const (
	codexBinaryEnv       = "AGENT_NOTIFY_CODEX_BIN"
	codexBinaryName      = "codex.exe"
	codexBinaryNamePlain = "codex"
	codexInstallSubdir   = "OpenAI/Codex/bin"
)

type codexBinaryCandidate struct {
	path    string
	modTime time.Time
}

func resolveCodexBinary(explicit string) (string, error) {
	if binary := strings.TrimSpace(explicit); binary != "" {
		return binary, nil
	}
	if binary := strings.TrimSpace(os.Getenv(codexBinaryEnv)); binary != "" {
		return binary, nil
	}
	if binary, err := exec.LookPath(codexBinaryNamePlain); err == nil {
		return binary, nil
	}
	if binary := findInstalledCodexCLI(); binary != "" {
		return binary, nil
	}
	return "", errors.New("找不到 codex 命令，请确认 Codex CLI 已安装")
}

func findInstalledCodexCLI() string {
	roots := codexInstallRoots()
	candidates := make([]codexBinaryCandidate, 0, len(roots))
	for _, root := range roots {
		candidates = appendCodexBinaryCandidates(candidates, root)
	}
	if len(candidates) == 0 {
		return ""
	}
	sort.Slice(candidates, func(i, j int) bool {
		if candidates[i].modTime.Equal(candidates[j].modTime) {
			return candidates[i].path < candidates[j].path
		}
		return candidates[i].modTime.After(candidates[j].modTime)
	})
	return candidates[0].path
}

func codexInstallRoots() []string {
	var roots []string
	add := func(root string) {
		root = strings.TrimSpace(root)
		if root == "" {
			return
		}
		root = filepath.Clean(root)
		for _, existing := range roots {
			if strings.EqualFold(existing, root) {
				return
			}
		}
		roots = append(roots, root)
	}

	if localAppData := strings.TrimSpace(os.Getenv("LOCALAPPDATA")); localAppData != "" {
		add(filepath.Join(localAppData, filepath.FromSlash(codexInstallSubdir)))
	}
	if cacheDir, err := os.UserCacheDir(); err == nil {
		add(filepath.Join(cacheDir, filepath.FromSlash(codexInstallSubdir)))
	}
	return roots
}

func appendCodexBinaryCandidates(candidates []codexBinaryCandidate, root string) []codexBinaryCandidate {
	add := func(path string) {
		if path == "" {
			return
		}
		info, err := os.Stat(path)
		if err != nil || !info.Mode().IsRegular() {
			return
		}
		candidates = append(candidates, codexBinaryCandidate{
			path:    path,
			modTime: info.ModTime(),
		})
	}

	for _, name := range []string{codexBinaryName, codexBinaryNamePlain} {
		add(filepath.Join(root, name))
	}
	entries, err := os.ReadDir(root)
	if err != nil {
		return candidates
	}
	for _, entry := range entries {
		if !entry.IsDir() {
			continue
		}
		for _, name := range []string{codexBinaryName, codexBinaryNamePlain} {
			add(filepath.Join(root, entry.Name(), name))
		}
	}
	return candidates
}
