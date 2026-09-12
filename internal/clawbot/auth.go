package clawbot

import (
	"context"
	"encoding/json"
	"fmt"
	"io"
	"net/http"
	"net/url"
	"os"
	"path/filepath"
	"strings"
	"time"

	"github.com/srafyhucl-cpu/agent-notify/internal/config"
)

const (
	StatusWait      = "wait"
	StatusScanned   = "scaned"
	StatusConfirmed = "confirmed"
	StatusExpired   = "expired"
)

// AuthClient handles the QR login flow.
type AuthClient struct {
	baseURL    string
	httpClient *http.Client
}

// NewAuthClient creates a client for an iLink-compatible endpoint.
func NewAuthClient(baseURL string) *AuthClient {
	baseURL = strings.TrimRight(strings.TrimSpace(baseURL), "/")
	if baseURL == "" {
		baseURL = DefaultBaseURL
	}
	return &AuthClient{
		baseURL:    baseURL,
		httpClient: &http.Client{Timeout: 45 * time.Second},
	}
}

// FetchQRCode starts a new ClawBot login session.
func (c *AuthClient) FetchQRCode(ctx context.Context) (QRCodeResponse, error) {
	endpoint := c.baseURL + "/ilink/bot/get_bot_qrcode?bot_type=3"
	var result QRCodeResponse
	if err := c.getJSON(ctx, endpoint, &result); err != nil {
		return result, fmt.Errorf("clawbot: fetch qr code: %w", err)
	}
	if result.Ret != 0 || result.QRCode == "" {
		return result, fmt.Errorf("clawbot: invalid qr response: ret=%d", result.Ret)
	}
	return result, nil
}

// PollQRStatus waits until the QR code is confirmed or expires. Transient
// network failures back off instead of spinning the login endpoint.
func (c *AuthClient) PollQRStatus(ctx context.Context, qrCode string, onStatus func(string)) (Credentials, error) {
	endpoint := c.baseURL + "/ilink/bot/get_qrcode_status?qrcode=" + url.QueryEscape(qrCode)
	delay := time.Second
	lastStatus := ""
	for {
		select {
		case <-ctx.Done():
			return Credentials{}, ctx.Err()
		default:
		}

		pollCtx, cancel := context.WithTimeout(ctx, 40*time.Second)
		var result QRStatusResponse
		err := c.getJSON(pollCtx, endpoint, &result)
		cancel()

		if err != nil {
			select {
			case <-ctx.Done():
				return Credentials{}, ctx.Err()
			case <-time.After(delay):
			}
			if delay < 5*time.Second {
				delay *= 2
				if delay > 5*time.Second {
					delay = 5 * time.Second
				}
			}
			continue
		}
		delay = time.Second

		if result.Ret != 0 {
			return Credentials{}, fmt.Errorf("clawbot: qr status failed: ret=%d errmsg=%s", result.Ret, result.ErrMsg)
		}
		if onStatus != nil && result.Status != lastStatus {
			lastStatus = result.Status
			onStatus(result.Status)
		}

		switch result.Status {
		case StatusConfirmed:
			creds := Credentials{
				BotToken:    result.BotToken,
				ILinkBotID:  result.ILinkBotID,
				BaseURL:     result.BaseURL,
				ILinkUserID: result.ILinkUserID,
			}
			if err := validateCredentials(creds); err != nil {
				return Credentials{}, fmt.Errorf("clawbot: confirmed login is incomplete: %w", err)
			}
			if creds.BaseURL == "" {
				creds.BaseURL = c.baseURL
			}
			return creds, nil
		case StatusExpired:
			return Credentials{}, fmt.Errorf("clawbot: qr code expired")
		}
	}
}

func (c *AuthClient) getJSON(ctx context.Context, endpoint string, result any) error {
	req, err := http.NewRequestWithContext(ctx, http.MethodGet, endpoint, nil)
	if err != nil {
		return err
	}
	req.Header.Set("Accept", "application/json")
	req.Header.Set("User-Agent", "Agent-notify")

	resp, err := c.httpClient.Do(req)
	if err != nil {
		return err
	}
	defer resp.Body.Close()
	data, err := io.ReadAll(resp.Body)
	if err != nil {
		return err
	}
	if resp.StatusCode != http.StatusOK {
		return fmt.Errorf("HTTP %d: %s", resp.StatusCode, strings.TrimSpace(string(data)))
	}
	if err := json.Unmarshal(data, result); err != nil {
		return err
	}
	return nil
}

// CredentialsPath returns the credential file path.
func CredentialsPath() string {
	return config.GetPaths().CredentialFile
}

func validateCredentials(creds Credentials) error {
	if strings.TrimSpace(creds.BotToken) == "" {
		return fmt.Errorf("bot token is empty")
	}
	if strings.TrimSpace(creds.ILinkBotID) == "" {
		return fmt.Errorf("bot id is empty")
	}
	if strings.TrimSpace(creds.ILinkUserID) == "" {
		return fmt.Errorf("recipient user id is empty")
	}
	return nil
}

// HasCredentials reports whether a usable credential file exists.
func HasCredentials() bool {
	creds, err := LoadCredentials()
	return err == nil && validateCredentials(creds) == nil
}

// LoadCredentials reads the saved ClawBot credentials.
func LoadCredentials() (Credentials, error) {
	path := CredentialsPath()
	data, err := os.ReadFile(path)
	if err != nil {
		return Credentials{}, err
	}
	var creds Credentials
	if err := json.Unmarshal(data, &creds); err != nil {
		return Credentials{}, fmt.Errorf("clawbot: decode credentials: %w", err)
	}
	if err := validateCredentials(creds); err != nil {
		return Credentials{}, fmt.Errorf("clawbot: invalid credentials: %w", err)
	}
	if creds.BaseURL == "" {
		creds.BaseURL = DefaultBaseURL
	}
	return creds, nil
}

// SaveCredentials writes credentials atomically with restrictive permissions.
func SaveCredentials(creds Credentials) error {
	if err := validateCredentials(creds); err != nil {
		return fmt.Errorf("clawbot: refusing to save incomplete credentials: %w", err)
	}
	if strings.TrimSpace(creds.BaseURL) == "" {
		creds.BaseURL = DefaultBaseURL
	}

	path := CredentialsPath()
	if err := os.MkdirAll(filepath.Dir(path), 0700); err != nil {
		return err
	}
	data, err := json.MarshalIndent(creds, "", "  ")
	if err != nil {
		return err
	}
	tmp := path + ".tmp"
	if err := os.WriteFile(tmp, append(data, '\n'), 0600); err != nil {
		return err
	}
	_ = os.Remove(path)
	if err := os.Rename(tmp, path); err != nil {
		_ = os.Remove(tmp)
		return err
	}
	return nil
}

// DeleteCredentials removes the saved login.
func DeleteCredentials() error {
	err := os.Remove(CredentialsPath())
	if err != nil && !os.IsNotExist(err) {
		return err
	}
	return nil
}

// GetStatus returns a secret-free summary for the CLI and desktop UI.
func GetStatus() Status {
	path := CredentialsPath()
	creds, err := LoadCredentials()
	if err != nil {
		return Status{Path: path}
	}
	return Status{
		LoggedIn:   true,
		Path:       path,
		BaseURL:    creds.BaseURL,
		ILinkBotID: creds.ILinkBotID,
		UserHint:   maskHint(creds.ILinkUserID),
	}
}

func maskHint(value string) string {
	runes := []rune(strings.TrimSpace(value))
	if len(runes) <= 8 {
		return strings.Repeat("*", len(runes))
	}
	return string(runes[:4]) + "..." + string(runes[len(runes)-4:])
}
