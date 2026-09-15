//go:build windows

package update

import (
	"context"
	"errors"
	"fmt"
	"io"
	"net/http"
	"net/url"
	"os"
	"runtime"
	"strings"
)

const (
	maxArchiveBytes   = int64(100 << 20)
	maxChecksumsBytes = int64(1 << 20)
)

func (client *Client) readURL(ctx context.Context, rawURL string, limit int64, accept string) ([]byte, error) {
	parsed, err := url.Parse(rawURL)
	if err != nil || parsed.Scheme == "" || parsed.Host == "" {
		return nil, fmt.Errorf("更新地址无效：%q", rawURL)
	}
	request, err := http.NewRequestWithContext(ctx, http.MethodGet, rawURL, nil)
	if err != nil {
		return nil, err
	}
	setRequestHeaders(request, client.Token, accept)

	httpClient := client.httpClient()
	response, err := httpClient.Do(request)
	if err != nil {
		return nil, err
	}
	defer response.Body.Close()
	if response.StatusCode < 200 || response.StatusCode >= 300 {
		return nil, responseError(response)
	}
	body, err := io.ReadAll(io.LimitReader(response.Body, limit+1))
	if err != nil {
		return nil, err
	}
	if int64(len(body)) > limit {
		return nil, fmt.Errorf("响应超过允许大小 %d 字节", limit)
	}
	return body, nil
}

func (client *Client) download(ctx context.Context, rawURL, destination string, limit int64) error {
	parsed, err := url.Parse(rawURL)
	if err != nil || parsed.Scheme == "" || parsed.Host == "" {
		return fmt.Errorf("更新地址无效：%q", rawURL)
	}
	request, err := http.NewRequestWithContext(ctx, http.MethodGet, rawURL, nil)
	if err != nil {
		return err
	}
	setRequestHeaders(request, client.Token, "application/octet-stream")

	response, err := client.httpClient().Do(request)
	if err != nil {
		return err
	}
	defer response.Body.Close()
	if response.StatusCode < 200 || response.StatusCode >= 300 {
		return responseError(response)
	}

	temporary := destination + ".part"
	file, err := os.OpenFile(temporary, os.O_CREATE|os.O_TRUNC|os.O_WRONLY, 0600)
	if err != nil {
		return err
	}
	written, copyErr := io.Copy(file, io.LimitReader(response.Body, limit+1))
	closeErr := file.Close()
	if copyErr != nil {
		_ = os.Remove(temporary)
		return copyErr
	}
	if closeErr != nil {
		_ = os.Remove(temporary)
		return closeErr
	}
	if written > limit {
		_ = os.Remove(temporary)
		return fmt.Errorf("更新文件超过允许大小 %d 字节", limit)
	}
	if err := os.Rename(temporary, destination); err != nil {
		_ = os.Remove(temporary)
		return err
	}
	return nil
}

func (client *Client) httpClient() *http.Client {
	if client.HTTPClient != nil {
		return client.HTTPClient
	}
	return &http.Client{Timeout: downloadTimeout}
}

func setRequestHeaders(request *http.Request, token, accept string) {
	request.Header.Set("User-Agent", "Agent-notify-updater/"+runtime.GOOS)
	if accept != "" {
		request.Header.Set("Accept", accept)
	}
	if token = strings.TrimSpace(token); token != "" {
		request.Header.Set("Authorization", "Bearer "+token)
	}
}

func responseError(response *http.Response) error {
	body, _ := io.ReadAll(io.LimitReader(response.Body, 4096))
	detail := strings.TrimSpace(string(body))
	if response.StatusCode == http.StatusNotFound {
		return errors.New("GitHub Release 不存在或仓库未公开")
	}
	if detail == "" {
		return fmt.Errorf("HTTP %d", response.StatusCode)
	}
	return fmt.Errorf("HTTP %d：%s", response.StatusCode, detail)
}
