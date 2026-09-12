package clawbot

import (
	"bytes"
	"context"
	"crypto/rand"
	"encoding/base64"
	"encoding/binary"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"net/http"
	"strings"
	"time"
)

const (
	defaultSendAttempts = 3
	initialRetryDelay   = 500 * time.Millisecond
)

// Client is a minimal iLink ClawBot API client.
type Client struct {
	baseURL    string
	botToken   string
	botID      string
	userID     string
	httpClient *http.Client
	wechatUIN  string
	attempts   int
}

type httpStatusError struct {
	status int
	body   string
}

func (e *httpStatusError) Error() string {
	return fmt.Sprintf("clawbot: HTTP %d: %s", e.status, strings.TrimSpace(e.body))
}

// NewClient creates a client from saved credentials.
func NewClient(creds Credentials) (*Client, error) {
	if err := validateCredentials(creds); err != nil {
		return nil, fmt.Errorf("clawbot: %w", err)
	}
	return newClientWithBaseURL(creds, creds.BaseURL), nil
}

func newClientWithBaseURL(creds Credentials, baseURL string) *Client {
	baseURL = strings.TrimRight(strings.TrimSpace(baseURL), "/")
	if baseURL == "" {
		baseURL = DefaultBaseURL
	}
	return &Client{
		baseURL:    baseURL,
		botToken:   creds.BotToken,
		botID:      creds.ILinkBotID,
		userID:     creds.ILinkUserID,
		httpClient: &http.Client{Timeout: 20 * time.Second},
		wechatUIN:  randomWechatUIN(),
		attempts:   defaultSendAttempts,
	}
}

// SendText sends one plain-text message to the account that completed ClawBot login.
func (c *Client) SendText(ctx context.Context, text string) error {
	if strings.TrimSpace(text) == "" {
		return fmt.Errorf("clawbot: message text is empty")
	}

	payload := sendMessageRequest{
		Msg: sendMessage{
			FromUserID:   c.botID,
			ToUserID:     c.userID,
			ClientID:     randomClientID(),
			MessageType:  MessageTypeBot,
			MessageState: MessageStateFinish,
			ItemList: []messageItem{{
				Type:     ItemTypeText,
				TextItem: &textItem{Text: text},
			}},
		},
	}

	attempts := c.attempts
	if attempts <= 0 {
		attempts = defaultSendAttempts
	}
	delay := initialRetryDelay
	var lastErr error
	for attempt := 1; attempt <= attempts; attempt++ {
		err := c.sendOnce(ctx, payload)
		if err == nil {
			return nil
		}
		lastErr = err
		if attempt == attempts || !isRetryable(err) {
			break
		}

		timer := time.NewTimer(delay)
		select {
		case <-ctx.Done():
			timer.Stop()
			return ctx.Err()
		case <-timer.C:
		}
		delay *= 2
	}
	return lastErr
}

func (c *Client) sendOnce(ctx context.Context, payload sendMessageRequest) error {
	var resp sendMessageResponse
	if err := c.postJSON(ctx, "/ilink/bot/sendmessage", payload, &resp); err != nil {
		return err
	}
	if resp.Ret != 0 {
		return fmt.Errorf("clawbot: send failed: ret=%d errmsg=%s", resp.Ret, resp.ErrMsg)
	}
	return nil
}

func (c *Client) postJSON(ctx context.Context, path string, body any, result any) error {
	data, err := json.Marshal(body)
	if err != nil {
		return fmt.Errorf("clawbot: marshal request: %w", err)
	}
	req, err := http.NewRequestWithContext(ctx, http.MethodPost, c.baseURL+path, bytes.NewReader(data))
	if err != nil {
		return fmt.Errorf("clawbot: create request: %w", err)
	}
	req.Header.Set("Content-Type", "application/json")
	req.Header.Set("AuthorizationType", "ilink_bot_token")
	req.Header.Set("Authorization", "Bearer "+c.botToken)
	req.Header.Set("X-WECHAT-UIN", c.wechatUIN)

	resp, err := c.httpClient.Do(req)
	if err != nil {
		return fmt.Errorf("clawbot: http request: %w", err)
	}
	defer resp.Body.Close()

	respData, err := io.ReadAll(resp.Body)
	if err != nil {
		return fmt.Errorf("clawbot: read response: %w", err)
	}
	if resp.StatusCode != http.StatusOK {
		return &httpStatusError{status: resp.StatusCode, body: string(respData)}
	}
	if err := json.Unmarshal(respData, result); err != nil {
		return fmt.Errorf("clawbot: decode response: %w", err)
	}
	return nil
}

func isRetryable(err error) bool {
	if err == nil {
		return false
	}
	if errors.Is(err, context.Canceled) || errors.Is(err, context.DeadlineExceeded) {
		return false
	}
	var statusErr *httpStatusError
	if errors.As(err, &statusErr) {
		return statusErr.status == http.StatusRequestTimeout ||
			statusErr.status == http.StatusTooManyRequests ||
			statusErr.status >= 500
	}
	return true
}

// Probe verifies that the configured ClawBot endpoint is reachable. Any HTTP
// response proves reachability; DNS/TLS/connection failures are returned.
func Probe(ctx context.Context, baseURL string) error {
	baseURL = strings.TrimRight(strings.TrimSpace(baseURL), "/")
	if baseURL == "" {
		baseURL = DefaultBaseURL
	}
	req, err := http.NewRequestWithContext(ctx, http.MethodHead, baseURL, nil)
	if err != nil {
		return err
	}
	req.Header.Set("User-Agent", "Agent-notify")
	client := &http.Client{Timeout: 8 * time.Second}
	resp, err := client.Do(req)
	if err != nil {
		return err
	}
	_ = resp.Body.Close()
	return nil
}

func randomClientID() string {
	var b [16]byte
	_, _ = rand.Read(b[:])
	return hex.EncodeToString(b[:])
}

func randomWechatUIN() string {
	var n uint32
	if err := binary.Read(rand.Reader, binary.LittleEndian, &n); err != nil {
		n = uint32(time.Now().UnixNano())
	}
	return base64.StdEncoding.EncodeToString([]byte(fmt.Sprintf("%d", n)))
}
