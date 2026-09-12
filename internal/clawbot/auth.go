package clawbot

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
	"sync"
	"time"

	"github.com/srafyhucl-cpu/agent-notify/internal/config"
)

// QR status values defined by the iLink ClawBot 2.4.6 protocol.
const (
	StatusWait            = "wait"
	StatusScanned         = "scaned"
	StatusConfirmed       = "confirmed"
	StatusScannedRedirect = "scaned_but_redirect"
	StatusBindedRedirect  = "binded_redirect"
	StatusNeedVerifyCode  = "need_verifycode"
	StatusVerifyBlocked   = "verify_code_blocked"
	StatusExpired         = "expired"
)

const (
	qrStatusTimeout  = 35 * time.Second
	maxQRRefresh     = 3
	qrPollRetryStart = time.Second
	qrPollRetryMax   = 5 * time.Second
)

var (
	// ErrQRCodeExpired means the QR code must be fetched again.
	ErrQRCodeExpired = errors.New("clawbot: 二维码已过期")
	// ErrVerifyBlocked means the pairing code was wrong too many times.
	ErrVerifyBlocked = errors.New("clawbot: 数字配对码错误次数过多")
	// ErrAlreadyBound means the account is already bound but no usable local
	// token exists, so the caller must start a new QR login.
	ErrAlreadyBound = errors.New("clawbot: 该微信已绑定过本机，但本地没有可用凭据")
)

// PollOptions customizes the QR status loop.
type PollOptions struct {
	// OnStatus receives each new protocol status string.
	OnStatus func(string)
	// VerifyCode is called when WeChat asks for the numeric pairing code.
	// retry is true when a previous code was rejected.
	VerifyCode func(retry bool) (string, error)
}

// LoginOptions customizes the full ClawBot login flow.
type LoginOptions struct {
	// OnQRCode receives every QR code that must be scanned. The same login
	// session may present up to three codes when earlier ones expire.
	OnQRCode func(QRCodeResponse) error
	// OnStatus receives each new protocol status string.
	OnStatus func(string)
	// VerifyCode is called when WeChat asks for the numeric pairing code.
	VerifyCode func(retry bool) (string, error)
	// LocalTokens overrides local_token_list; nil means "use the saved token".
	LocalTokens []string
}

// AuthClient handles the QR login flow.
type AuthClient struct {
	baseURL    string
	httpClient *http.Client
}

// NewAuthClient creates a client for the iLink login endpoint.
func NewAuthClient(baseURL string) *AuthClient {
	baseURL = strings.TrimRight(strings.TrimSpace(baseURL), "/")
	if baseURL == "" {
		baseURL = DefaultBaseURL
	}
	return &AuthClient{
		baseURL:    baseURL,
		httpClient: &http.Client{Timeout: qrStatusTimeout + 10*time.Second},
	}
}

// FetchQRCode starts a new ClawBot login session. The POST form carries the
// local token list so the server can tell whether this client already bound
// the scanned account; the GET form remains only as a fallback for servers
// that still answer the older shape.
func (c *AuthClient) FetchQRCode(ctx context.Context, localTokens []string) (QRCodeResponse, error) {
	tokens := uniqueTokens(localTokens)
	var result QRCodeResponse
	body := qrCodeRequest{LocalTokenList: tokens, BaseInfo: newBaseInfo()}
	if err := c.doJSON(ctx, http.MethodPost, c.baseURL+"/ilink/bot/get_bot_qrcode?bot_type=3", body, &result); err != nil {
		return QRCodeResponse{}, fmt.Errorf("clawbot: fetch qr code: %w", err)
	}
	if err := checkAPIStatus("get_bot_qrcode", result.Ret, result.ErrCode, result.ErrMsg); err != nil {
		return result, err
	}
	if strings.TrimSpace(result.QRCode) != "" {
		return result, nil
	}

	// Old servers answer POST without a code and expose the GET shape only.
	var fallback QRCodeResponse
	if err := c.doJSON(ctx, http.MethodGet, c.baseURL+"/ilink/bot/get_bot_qrcode?bot_type=3", nil, &fallback); err != nil {
		return result, fmt.Errorf("clawbot: fetch qr code: %w", err)
	}
	if err := checkAPIStatus("get_bot_qrcode", fallback.Ret, fallback.ErrCode, fallback.ErrMsg); err != nil {
		return fallback, err
	}
	if strings.TrimSpace(fallback.QRCode) == "" {
		return fallback, fmt.Errorf("clawbot: qr response missing qrcode")
	}
	return fallback, nil
}

// Login runs the whole QR login flow: fetch a code, wait for the scan, ask for
// the numeric pairing code when WeChat requests it, and refresh the code when
// it expires or the pairing code is blocked.
func Login(ctx context.Context, options LoginOptions) (Credentials, error) {
	client := NewAuthClient(DefaultBaseURL)
	tokens := options.LocalTokens
	if tokens == nil {
		tokens = savedTokenList()
	}

	var lastErr error
	for refresh := 0; refresh < maxQRRefresh; refresh++ {
		qr, err := client.FetchQRCode(ctx, tokens)
		if err != nil {
			return Credentials{}, err
		}
		if options.OnQRCode != nil {
			if err := options.OnQRCode(qr); err != nil {
				return Credentials{}, err
			}
		}

		credentials, err := client.PollQRStatus(ctx, qr.QRCode, PollOptions{
			OnStatus:   options.OnStatus,
			VerifyCode: options.VerifyCode,
		})
		if err == nil {
			return credentials, nil
		}
		lastErr = err
		if ctx.Err() != nil {
			return Credentials{}, ctx.Err()
		}
		if errors.Is(err, ErrAlreadyBound) {
			tokens = nil
		}
		if !errors.Is(err, ErrQRCodeExpired) && !errors.Is(err, ErrVerifyBlocked) && !errors.Is(err, ErrAlreadyBound) {
			return Credentials{}, err
		}
	}
	if lastErr == nil {
		lastErr = errors.New("clawbot: login incomplete")
	}
	return Credentials{}, fmt.Errorf("clawbot: 二维码多次失效或登录失败，请稍后重试: %w", lastErr)
}

// PollQRStatus waits until the QR code is confirmed, refreshed, or rejected.
// Transient failures back off instead of aborting the login.
func (c *AuthClient) PollQRStatus(ctx context.Context, qrCode string, options PollOptions) (Credentials, error) {
	qrCode = strings.TrimSpace(qrCode)
	if qrCode == "" {
		return Credentials{}, fmt.Errorf("clawbot: qr code is empty")
	}

	statusBase := c.baseURL
	confirmedBase := c.baseURL
	verifyCode := ""
	lastStatus := ""
	delay := qrPollRetryStart

	for {
		if err := ctx.Err(); err != nil {
			return Credentials{}, err
		}

		result, err := c.pollQRStatusOnce(ctx, statusBase, qrCode, verifyCode)
		if err != nil {
			if ctx.Err() != nil {
				return Credentials{}, ctx.Err()
			}
			if err := sleepContext(ctx, delay); err != nil {
				return Credentials{}, err
			}
			if delay < qrPollRetryMax {
				delay *= 2
				if delay > qrPollRetryMax {
					delay = qrPollRetryMax
				}
			}
			continue
		}
		delay = qrPollRetryStart

		if err := checkAPIStatus("get_qrcode_status", result.Ret, result.ErrCode, result.ErrMsg); err != nil {
			return Credentials{}, err
		}

		status := strings.TrimSpace(result.Status)
		if status == "" && result.BindedRedirect {
			status = StatusBindedRedirect
		}
		if options.OnStatus != nil && status != "" && status != lastStatus {
			lastStatus = status
			options.OnStatus(status)
		}

		switch {
		case status == StatusConfirmed || strings.TrimSpace(result.BotToken) != "":
			credentials := Credentials{
				BotToken:    strings.TrimSpace(result.BotToken),
				ILinkBotID:  strings.TrimSpace(result.ILinkBotID),
				BaseURL:     strings.TrimSpace(result.BaseURL),
				ILinkUserID: strings.TrimSpace(result.ILinkUserID),
			}
			if credentials.BaseURL == "" {
				credentials.BaseURL = confirmedBase
			}
			if err := validateCredentials(credentials); err != nil {
				return Credentials{}, fmt.Errorf("clawbot: confirmed login is incomplete: %w", err)
			}
			return credentials, nil

		case status == StatusBindedRedirect || result.BindedRedirect:
			// The account already bound this client. Only reuse the hidden
			// local token when it is still usable.
			credentials, err := LoadCredentials()
			if err == nil && strings.TrimSpace(credentials.StaleAt) == "" {
				return credentials, nil
			}
			return Credentials{}, ErrAlreadyBound

		case status == StatusExpired:
			return Credentials{}, ErrQRCodeExpired

		case status == StatusVerifyBlocked:
			return Credentials{}, ErrVerifyBlocked

		case status == StatusScannedRedirect:
			host := strings.TrimSpace(result.RedirectHost)
			if host == "" {
				continue
			}
			statusBase = withScheme(host)
			confirmedBase = statusBase
			continue

		case status == StatusScanned:
			verifyCode = ""

		case status == StatusNeedVerifyCode || result.NeedVerifyCode:
			if options.VerifyCode == nil {
				return Credentials{}, fmt.Errorf("clawbot: 微信要求输入数字配对码，但当前环境无法输入")
			}
			code, err := options.VerifyCode(verifyCode != "")
			if err != nil {
				return Credentials{}, err
			}
			code = strings.TrimSpace(code)
			if code == "" {
				return Credentials{}, fmt.Errorf("clawbot: 数字配对码为空")
			}
			verifyCode = code
		}
	}
}

func (c *AuthClient) pollQRStatusOnce(ctx context.Context, baseURL, qrCode, verifyCode string) (QRStatusResponse, error) {
	endpoint := strings.TrimRight(baseURL, "/") + "/ilink/bot/get_qrcode_status?qrcode=" + url.QueryEscape(qrCode)
	if verifyCode != "" {
		endpoint += "&verify_code=" + url.QueryEscape(verifyCode)
	}

	pollCtx, cancel := context.WithTimeout(ctx, qrStatusTimeout)
	defer cancel()

	var result QRStatusResponse
	if err := c.doJSON(pollCtx, http.MethodGet, endpoint, nil, &result); err != nil {
		if ctx.Err() != nil {
			return QRStatusResponse{}, ctx.Err()
		}
		if errors.Is(err, context.DeadlineExceeded) {
			// A long poll that returns nothing simply means "still waiting".
			return QRStatusResponse{Status: StatusWait}, nil
		}
		return QRStatusResponse{}, err
	}
	return result, nil
}

func (c *AuthClient) doJSON(ctx context.Context, method, endpoint string, body any, result any) error {
	var reader io.Reader
	if body != nil {
		data, err := json.Marshal(body)
		if err != nil {
			return err
		}
		reader = strings.NewReader(string(data))
	}

	req, err := http.NewRequestWithContext(ctx, method, endpoint, reader)
	if err != nil {
		return err
	}
	req.Header.Set("Accept", "application/json")
	req.Header.Set("User-Agent", "Agent-notify")
	req.Header.Set("X-WECHAT-UIN", randomWechatUIN())
	req.Header.Set("iLink-App-Id", AppID)
	req.Header.Set("iLink-App-ClientVersion", AppClientVersion)
	if body != nil {
		req.Header.Set("Content-Type", "application/json")
	}

	resp, err := c.httpClient.Do(req)
	if err != nil {
		return err
	}
	defer resp.Body.Close()

	data, err := io.ReadAll(resp.Body)
	if err != nil {
		return err
	}
	if resp.StatusCode < 200 || resp.StatusCode >= 300 {
		return fmt.Errorf("HTTP %d: %s", resp.StatusCode, strings.TrimSpace(string(data)))
	}
	if err := json.Unmarshal(data, result); err != nil {
		return err
	}
	return nil
}

func withScheme(host string) string {
	host = strings.TrimSpace(host)
	if strings.HasPrefix(host, "http://") || strings.HasPrefix(host, "https://") {
		return strings.TrimRight(host, "/")
	}
	return "https://" + strings.TrimRight(host, "/")
}

func uniqueTokens(tokens []string) []string {
	seen := make(map[string]struct{}, len(tokens))
	result := make([]string, 0, len(tokens))
	for _, token := range tokens {
		token = strings.TrimSpace(token)
		if token == "" {
			continue
		}
		if _, ok := seen[token]; ok {
			continue
		}
		seen[token] = struct{}{}
		result = append(result, token)
		if len(result) == 10 {
			break
		}
	}
	return result
}

func savedTokenList() []string {
	credentials, err := LoadCredentials()
	if err != nil || strings.TrimSpace(credentials.StaleAt) != "" {
		return nil
	}
	return uniqueTokens([]string{credentials.BotToken})
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

// HasCredentials reports whether a stored login exists.
func HasCredentials() bool {
	credentials, err := LoadCredentials()
	return err == nil && validateCredentials(credentials) == nil
}

var credentialsMu sync.Mutex

// LoadCredentials reads the saved ClawBot credentials.
func LoadCredentials() (Credentials, error) {
	credentialsMu.Lock()
	defer credentialsMu.Unlock()
	return loadCredentials()
}

func loadCredentials() (Credentials, error) {
	data, err := os.ReadFile(CredentialsPath())
	if err != nil {
		return Credentials{}, err
	}
	var credentials Credentials
	if err := json.Unmarshal(data, &credentials); err != nil {
		return Credentials{}, fmt.Errorf("clawbot: decode credentials: %w", err)
	}
	if err := validateCredentials(credentials); err != nil {
		return Credentials{}, fmt.Errorf("clawbot: invalid credentials: %w", err)
	}
	if strings.TrimSpace(credentials.BaseURL) == "" {
		credentials.BaseURL = DefaultBaseURL
	}
	return credentials, nil
}

// SaveCredentials writes credentials atomically with restrictive permissions.
func SaveCredentials(creds Credentials) error {
	credentialsMu.Lock()
	defer credentialsMu.Unlock()
	return saveCredentials(creds)
}

func saveCredentials(creds Credentials) error {
	if err := validateCredentials(creds); err != nil {
		return fmt.Errorf("clawbot: refusing to save incomplete credentials: %w", err)
	}
	if strings.TrimSpace(creds.BaseURL) == "" {
		creds.BaseURL = DefaultBaseURL
	}

	if previous, err := loadCredentials(); err == nil && strings.TrimSpace(previous.ILinkBotID) != "" && previous.ILinkBotID != creds.ILinkBotID {
		// A cursor and context token are scoped to one bot account. Never
		// carry them into a different account after re-login.
		creds.GetUpdatesBuf = ""
		creds.ContextToken = ""
		creds.ContextUserID = ""
	}
	if strings.TrimSpace(creds.ContextUserID) != strings.TrimSpace(creds.ILinkUserID) {
		creds.ContextToken = ""
		creds.ContextUserID = ""
	}
	if strings.TrimSpace(creds.StaleAt) != "" {
		// A stale login must never retain context that would make the UI
		// believe proactive sending still works.
		creds.ContextToken = ""
		creds.ContextUserID = ""
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
	if err := os.Rename(tmp, path); err != nil {
		_ = os.Remove(tmp)
		return err
	}
	return nil
}

// DeleteCredentials removes the saved login and its message context.
func DeleteCredentials() error {
	credentialsMu.Lock()
	defer credentialsMu.Unlock()
	err := os.Remove(CredentialsPath())
	if err != nil && !os.IsNotExist(err) {
		return err
	}
	return nil
}

// updateCredentials applies one atomic read-modify-write to the credential
// file so a background session poll cannot overwrite a fresh QR login.
func updateCredentials(apply func(*Credentials) error) error {
	credentialsMu.Lock()
	defer credentialsMu.Unlock()

	credentials, err := loadCredentials()
	if err != nil {
		return err
	}
	if err := apply(&credentials); err != nil {
		return err
	}
	return saveCredentials(credentials)
}

// GetStatus returns a secret-free summary for the CLI and desktop UI.
func GetStatus() Status {
	path := CredentialsPath()
	credentials, err := LoadCredentials()
	if err != nil {
		return Status{Path: path}
	}
	status := Status{
		LoggedIn:   true,
		Path:       path,
		BaseURL:    credentials.BaseURL,
		ILinkBotID: credentials.ILinkBotID,
		Stale:      strings.TrimSpace(credentials.StaleAt) != "",
	}
	hint := credentials.ContextUserID
	if strings.TrimSpace(hint) == "" {
		hint = credentials.ILinkUserID
	}
	status.UserHint = maskHint(hint)
	status.SessionReady = !status.Stale &&
		strings.TrimSpace(credentials.ContextToken) != "" &&
		strings.TrimSpace(credentials.ContextUserID) == strings.TrimSpace(credentials.ILinkUserID)
	return status
}

func maskHint(value string) string {
	runes := []rune(strings.TrimSpace(value))
	if len(runes) <= 8 {
		return strings.Repeat("*", len(runes))
	}
	return string(runes[:4]) + "..." + string(runes[len(runes)-4:])
}
