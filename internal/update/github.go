//go:build windows

package update

import (
	"fmt"
	"net/url"
	"strings"
)

type githubRelease struct {
	TagName    string               `json:"tag_name"`
	Body       string               `json:"body"`
	Draft      bool                 `json:"draft"`
	Prerelease bool                 `json:"prerelease"`
	Assets     []githubReleaseAsset `json:"assets"`
}

type githubReleaseAsset struct {
	Name               string `json:"name"`
	URL                string `json:"url"`
	BrowserDownloadURL string `json:"browser_download_url"`
}

func findAsset(assets []githubReleaseAsset, name string) (githubReleaseAsset, bool) {
	for _, asset := range assets {
		if strings.EqualFold(strings.TrimSpace(asset.Name), name) {
			return asset, true
		}
	}
	return githubReleaseAsset{}, false
}

func validateRepository(repository string) error {
	parts := strings.Split(repository, "/")
	if len(parts) != 2 || strings.TrimSpace(parts[0]) == "" || strings.TrimSpace(parts[1]) == "" {
		return fmt.Errorf("GitHub 仓库格式无效：%q", repository)
	}
	return nil
}

func escapeRepository(repository string) string {
	parts := strings.Split(repository, "/")
	return url.PathEscape(strings.TrimSpace(parts[0])) + "/" + url.PathEscape(strings.TrimSpace(parts[1]))
}
