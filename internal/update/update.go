//go:build windows

// Package update implements Agent-notify's one-click GitHub Release updater.
package update

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"net/http"
	"net/url"
	"os"
	"path/filepath"
	"strings"
	"time"
)

const (
	defaultRepository = "srafyhucl-cpu/agent-notify-releases"
	defaultAPIBaseURL = "https://api.github.com"
	defaultWebBaseURL = "https://github.com"

	downloadTimeout = 3 * time.Minute

	releaseRootName = "Agent-notify"
	checksumName    = "SHA256SUMS.txt"
)

// ArtifactKind 表示 Release 中可用于升级的产物类型。
type ArtifactKind string

const (
	ArtifactInstaller ArtifactKind = "installer"
	ArtifactArchive   ArtifactKind = "archive"
)

// Release describes one installable GitHub Release.
type Release struct {
	Version      string       `json:"version"`
	TagName      string       `json:"tagName"`
	Notes        string       `json:"notes,omitempty"`
	ArtifactKind ArtifactKind `json:"artifactKind,omitempty"`
	ArtifactURL  string       `json:"artifactURL,omitempty"`
	// ArchiveURL 仅用于兼容旧调用，Check 返回的新结果统一使用 ArtifactURL。
	ArchiveURL  string `json:"archiveURL,omitempty"`
	ChecksumURL string `json:"checksumURL"`
}

// PreparedUpdate is a verified installer or extracted archive in an isolated staging directory.
type PreparedUpdate struct {
	Version       string
	StageDir      string
	ArtifactPath  string
	InstallerPath string
	Kind          ArtifactKind
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
		fallback, available, fallbackErr := client.checkViaRedirect(ctx, repository, apiBaseURL, currentVersion)
		if fallbackErr == nil {
			return fallback, available, nil
		}
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

	installerName := installerAssetName(version)
	artifactKind := ArtifactInstaller
	artifact, ok := findAsset(latest.Assets, installerName)
	if !ok {
		archiveName := archiveAssetName(version)
		artifact, ok = findAsset(latest.Assets, archiveName)
		if !ok {
			return Release{}, false, fmt.Errorf("Release 缺少更新包：%s", archiveName)
		}
		artifactKind = ArtifactArchive
	}
	checksums, ok := findAsset(latest.Assets, checksumName)
	if !ok {
		return Release{}, false, fmt.Errorf("Release 缺少校验文件：%s", checksumName)
	}
	artifactURL := assetDownloadURL(artifact)
	checksumURL := assetDownloadURL(checksums)
	if artifactURL == "" || checksumURL == "" {
		return Release{}, false, errors.New("Release 下载地址不完整")
	}

	return Release{
		Version:      version,
		TagName:      strings.TrimSpace(latest.TagName),
		Notes:        strings.TrimSpace(latest.Body),
		ArtifactKind: artifactKind,
		ArtifactURL:  artifactURL,
		ChecksumURL:  checksumURL,
	}, true, nil
}

func assetDownloadURL(asset githubReleaseAsset) string {
	if value := strings.TrimSpace(asset.URL); value != "" {
		return value
	}
	return strings.TrimSpace(asset.BrowserDownloadURL)
}

func (client *Client) checkViaRedirect(ctx context.Context, repository, apiBaseURL, currentVersion string) (Release, bool, error) {
	webBaseURL := defaultWebBaseURL
	if strings.TrimRight(apiBaseURL, "/") != defaultAPIBaseURL {
		webBaseURL = strings.TrimRight(apiBaseURL, "/")
	}
	requestURL := webBaseURL + "/" + escapeRepository(repository) + "/releases/latest"
	request, err := http.NewRequestWithContext(ctx, http.MethodGet, requestURL, nil)
	if err != nil {
		return Release{}, false, err
	}
	setRequestHeaders(request, client.Token, "text/html")
	response, err := client.httpClient().Do(request)
	if err != nil {
		return Release{}, false, err
	}
	defer response.Body.Close()
	if response.StatusCode < 200 || response.StatusCode >= 300 {
		return Release{}, false, responseError(response)
	}
	finalURL := response.Request.URL
	index := strings.LastIndex(finalURL.Path, "/releases/tag/")
	if index < 0 {
		return Release{}, false, errors.New("最新 Release 地址无法解析")
	}
	tagName, err := url.PathUnescape(strings.TrimPrefix(finalURL.Path[index:], "/releases/tag/"))
	if err != nil {
		return Release{}, false, err
	}
	version, ok := normalizeVersion(tagName)
	if !ok {
		return Release{}, false, fmt.Errorf("Release 标签版本无效：%q", tagName)
	}
	newer, err := isNewerVersion(version, currentVersion)
	if err != nil {
		return Release{}, false, err
	}
	if !newer {
		return Release{}, false, nil
	}
	archiveName := archiveAssetName(version)
	downloadBase := webBaseURL + "/" + escapeRepository(repository) + "/releases/download/" + url.PathEscape(tagName) + "/"
	return Release{
		Version:      version,
		TagName:      tagName,
		ArtifactKind: ArtifactArchive,
		ArtifactURL:  downloadBase + url.PathEscape(archiveName),
		ChecksumURL:  downloadBase + url.PathEscape(checksumName),
	}, true, nil
}

// Prepare downloads and verifies one Release into root.
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
	artifactKind, artifactURL, err := releaseArtifact(release)
	if err != nil {
		return PreparedUpdate{}, err
	}
	artifactName, err := artifactFileName(version, artifactKind)
	if err != nil {
		return PreparedUpdate{}, err
	}
	if strings.TrimSpace(release.ChecksumURL) == "" {
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

	artifactPath := filepath.Join(stageDir, artifactName)
	checksumsPath := filepath.Join(stageDir, checksumName)
	if err := client.download(ctx, release.ChecksumURL, checksumsPath, maxChecksumsBytes); err != nil {
		return PreparedUpdate{}, fmt.Errorf("下载校验文件失败：%w", err)
	}
	if err := client.download(ctx, artifactURL, artifactPath, maxArchiveBytes); err != nil {
		return PreparedUpdate{}, fmt.Errorf("下载更新包失败：%w", err)
	}
	if err := verifyChecksum(checksumsPath, artifactName, artifactPath); err != nil {
		return PreparedUpdate{}, err
	}
	if artifactKind == ArtifactInstaller {
		if err := validateWindowsExecutable(artifactPath); err != nil {
			return PreparedUpdate{}, err
		}
		if err := verifyArtifactSignature(ctx, artifactPath); err != nil {
			return PreparedUpdate{}, err
		}
		_ = os.Remove(checksumsPath)
		return PreparedUpdate{
			Version:       version,
			StageDir:      stageDir,
			ArtifactPath:  artifactPath,
			InstallerPath: artifactPath,
			Kind:          ArtifactInstaller,
		}, nil
	}

	extractDir := filepath.Join(stageDir, "extracted")
	if err := os.MkdirAll(extractDir, 0700); err != nil {
		return PreparedUpdate{}, fmt.Errorf("创建解压目录失败：%w", err)
	}
	if err := extractZip(artifactPath, extractDir); err != nil {
		return PreparedUpdate{}, err
	}

	releaseDir := filepath.Join(extractDir, releaseRootName)
	installerPath := filepath.Join(releaseDir, "install.ps1")
	executablePath := filepath.Join(releaseDir, "bin", "agent-notify.exe")
	versionPath := filepath.Join(releaseDir, "VERSION")
	if err := validatePreparedRelease(releaseDir, installerPath, executablePath, versionPath, version); err != nil {
		return PreparedUpdate{}, err
	}
	if err := validateWindowsExecutable(executablePath); err != nil {
		return PreparedUpdate{}, err
	}
	if err := verifyArtifactSignature(ctx, executablePath); err != nil {
		return PreparedUpdate{}, err
	}

	_ = os.Remove(checksumsPath)
	return PreparedUpdate{
		Version:       version,
		StageDir:      releaseDir,
		ArtifactPath:  artifactPath,
		InstallerPath: installerPath,
		Kind:          ArtifactArchive,
	}, nil
}

func releaseArtifact(release Release) (ArtifactKind, string, error) {
	kind := release.ArtifactKind
	artifactURL := strings.TrimSpace(release.ArtifactURL)
	if artifactURL == "" {
		artifactURL = strings.TrimSpace(release.ArchiveURL)
		if kind == "" {
			kind = ArtifactArchive
		}
	}
	if kind == "" {
		kind = ArtifactArchive
	}
	if kind != ArtifactInstaller && kind != ArtifactArchive {
		return "", "", fmt.Errorf("更新产物类型无效：%q", kind)
	}
	if artifactURL == "" {
		return "", "", errors.New("更新下载地址不完整")
	}
	return kind, artifactURL, nil
}

func artifactFileName(version string, kind ArtifactKind) (string, error) {
	switch kind {
	case ArtifactInstaller:
		return installerAssetName(version), nil
	case ArtifactArchive:
		return archiveAssetName(version), nil
	default:
		return "", fmt.Errorf("更新产物类型无效：%q", kind)
	}
}

func installerAssetName(version string) string {
	return fmt.Sprintf("Agent-notify-Setup-v%s.exe", version)
}

func archiveAssetName(version string) string {
	return fmt.Sprintf("Agent-notify-v%s.zip", version)
}

func validateWindowsExecutable(path string) error {
	file, err := os.Open(path)
	if err != nil {
		return err
	}
	defer file.Close()

	header := make([]byte, 2)
	if _, err := io.ReadFull(file, header); err != nil || header[0] != 'M' || header[1] != 'Z' {
		return errors.New("更新安装器不是有效的 Windows 程序")
	}
	return nil
}
