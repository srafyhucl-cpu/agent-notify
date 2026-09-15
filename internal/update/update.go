//go:build windows

// Package update implements Agent-notify's one-click GitHub Release updater.
package update

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"net/http"
	"os"
	"path/filepath"
	"strings"
	"time"
)

const (
	defaultRepository = "srafyhucl-cpu/agent-notify"
	defaultAPIBaseURL = "https://api.github.com"

	downloadTimeout = 3 * time.Minute

	releaseRootName = "Agent-notify"
	checksumName    = "SHA256SUMS.txt"
)

// Release describes one installable GitHub Release.
type Release struct {
	Version     string `json:"version"`
	TagName     string `json:"tagName"`
	Notes       string `json:"notes,omitempty"`
	ArchiveURL  string `json:"archiveURL"`
	ChecksumURL string `json:"checksumURL"`
}

// PreparedUpdate is a verified release extracted into an isolated staging directory.
type PreparedUpdate struct {
	Version       string
	StageDir      string
	InstallerPath string
}

// Client checks GitHub Releases and prepares verified update archives.
type Client struct {
	HTTPClient *http.Client
	Repository string
	APIBaseURL string
	Token      string
}

// NewClient returns the production updater client.
func NewClient() *Client {
	repository := strings.TrimSpace(os.Getenv("AGENT_NOTIFY_UPDATE_REPOSITORY"))
	if repository == "" {
		repository = defaultRepository
	}
	apiBaseURL := strings.TrimRight(strings.TrimSpace(os.Getenv("AGENT_NOTIFY_UPDATE_API_BASE")), "/")
	if apiBaseURL == "" {
		apiBaseURL = defaultAPIBaseURL
	}
	return &Client{
		HTTPClient: &http.Client{Timeout: downloadTimeout},
		Repository: repository,
		APIBaseURL: apiBaseURL,
		Token:      strings.TrimSpace(os.Getenv("AGENT_NOTIFY_GITHUB_TOKEN")),
	}
}

// Check returns the latest public Release when it is newer than currentVersion.
func (client *Client) Check(ctx context.Context, currentVersion string) (Release, bool, error) {
	if client == nil {
		client = NewClient()
	}
	if _, ok := normalizeVersion(currentVersion); !ok {
		return Release{}, false, fmt.Errorf("当前版本 %q 不支持自动更新", currentVersion)
	}
	httpClient := client.HTTPClient
	if httpClient == nil {
		httpClient = &http.Client{Timeout: downloadTimeout}
	}
	apiBaseURL := strings.TrimRight(strings.TrimSpace(client.APIBaseURL), "/")
	if apiBaseURL == "" {
		apiBaseURL = defaultAPIBaseURL
	}
	repository := strings.TrimSpace(client.Repository)
	if repository == "" {
		repository = defaultRepository
	}
	if err := validateRepository(repository); err != nil {
		return Release{}, false, err
	}

	requestURL := apiBaseURL + "/repos/" + escapeRepository(repository) + "/releases/latest"
	body, err := client.readURL(ctx, requestURL, maxChecksumsBytes, "application/vnd.github+json")
	if err != nil {
		return Release{}, false, fmt.Errorf("检查更新失败：%w", err)
	}

	var latest githubRelease
	if err := json.Unmarshal(body, &latest); err != nil {
		return Release{}, false, fmt.Errorf("解析 Release 信息失败：%w", err)
	}
	if latest.Draft || latest.Prerelease {
		return Release{}, false, errors.New("最新 Release 不是稳定版本")
	}
	version, ok := normalizeVersion(latest.TagName)
	if !ok {
		return Release{}, false, fmt.Errorf("Release 标签版本无效：%q", latest.TagName)
	}
	newer, err := isNewerVersion(version, currentVersion)
	if err != nil {
		return Release{}, false, err
	}
	if !newer {
		return Release{}, false, nil
	}

	archiveName := fmt.Sprintf("Agent-notify-v%s.zip", version)
	archive, ok := findAsset(latest.Assets, archiveName)
	if !ok {
		return Release{}, false, fmt.Errorf("Release 缺少更新包：%s", archiveName)
	}
	checksums, ok := findAsset(latest.Assets, checksumName)
	if !ok {
		return Release{}, false, fmt.Errorf("Release 缺少校验文件：%s", checksumName)
	}
	archiveURL := strings.TrimSpace(archive.URL)
	if archiveURL == "" {
		archiveURL = strings.TrimSpace(archive.BrowserDownloadURL)
	}
	checksumURL := strings.TrimSpace(checksums.URL)
	if checksumURL == "" {
		checksumURL = strings.TrimSpace(checksums.BrowserDownloadURL)
	}
	if archiveURL == "" || checksumURL == "" {
		return Release{}, false, errors.New("Release 下载地址不完整")
	}

	return Release{
		Version:     version,
		TagName:     strings.TrimSpace(latest.TagName),
		Notes:       strings.TrimSpace(latest.Body),
		ArchiveURL:  archiveURL,
		ChecksumURL: checksumURL,
	}, true, nil
}

// Prepare downloads, verifies, and extracts one Release into root.
func (client *Client) Prepare(ctx context.Context, release Release, root string) (PreparedUpdate, error) {
	if client == nil {
		client = NewClient()
	}
	version, ok := normalizeVersion(release.Version)
	if !ok {
		return PreparedUpdate{}, fmt.Errorf("更新版本无效：%q", release.Version)
	}
	root = strings.TrimSpace(root)
	if root == "" {
		return PreparedUpdate{}, errors.New("更新临时目录为空")
	}
	if strings.TrimSpace(release.ArchiveURL) == "" || strings.TrimSpace(release.ChecksumURL) == "" {
		return PreparedUpdate{}, errors.New("更新下载地址不完整")
	}

	if err := os.MkdirAll(root, 0700); err != nil {
		return PreparedUpdate{}, fmt.Errorf("创建更新目录失败：%w", err)
	}
	stageDir := filepath.Join(root, version)
	if err := os.RemoveAll(stageDir); err != nil {
		return PreparedUpdate{}, fmt.Errorf("清理旧更新目录失败：%w", err)
	}
	if err := os.MkdirAll(stageDir, 0700); err != nil {
		return PreparedUpdate{}, fmt.Errorf("创建版本目录失败：%w", err)
	}

	archiveName := fmt.Sprintf("Agent-notify-v%s.zip", version)
	archivePath := filepath.Join(stageDir, archiveName)
	checksumsPath := filepath.Join(stageDir, checksumName)
	if err := client.download(ctx, release.ChecksumURL, checksumsPath, maxChecksumsBytes); err != nil {
		return PreparedUpdate{}, fmt.Errorf("下载校验文件失败：%w", err)
	}
	if err := client.download(ctx, release.ArchiveURL, archivePath, maxArchiveBytes); err != nil {
		return PreparedUpdate{}, fmt.Errorf("下载更新包失败：%w", err)
	}
	if err := verifyChecksum(checksumsPath, archiveName, archivePath); err != nil {
		return PreparedUpdate{}, err
	}

	extractDir := filepath.Join(stageDir, "extracted")
	if err := os.MkdirAll(extractDir, 0700); err != nil {
		return PreparedUpdate{}, fmt.Errorf("创建解压目录失败：%w", err)
	}
	if err := extractZip(archivePath, extractDir); err != nil {
		return PreparedUpdate{}, err
	}

	releaseDir := filepath.Join(extractDir, releaseRootName)
	installerPath := filepath.Join(releaseDir, "install.ps1")
	executablePath := filepath.Join(releaseDir, "bin", "agent-notify.exe")
	versionPath := filepath.Join(releaseDir, "VERSION")
	if err := validatePreparedRelease(releaseDir, installerPath, executablePath, versionPath, version); err != nil {
		return PreparedUpdate{}, err
	}

	_ = os.Remove(archivePath)
	_ = os.Remove(checksumsPath)
	return PreparedUpdate{
		Version:       version,
		StageDir:      releaseDir,
		InstallerPath: installerPath,
	}, nil
}
