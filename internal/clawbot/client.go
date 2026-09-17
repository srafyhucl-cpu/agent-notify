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
	"net/http"
	"strings"
	"time"
)

const (
	defaultSendAttempts = 3
	sendTimeout         = 15 * time.Second
	initialRetryDelay   = 500 * time.Millisecond
	clientIDBytes       = 16
	endpointSendMessage = "/ilink/bot/sendmessage"
	endpointGetUpdates  = "/ilink/bot/getupdates"
)

var (
	// ErrStaleToken means the server rejected the saved bot token with ret/errcode -14.
	ErrStaleToken = errors.New("clawbot: 登录凭据已失效，请重新扫码登录")
	// ErrNoSession means no inbound message has established a context token yet.
	ErrNoSession = errors.New("clawbot: 尚未建立微信会话，请先给 ClawBot 发送一条消息")
	// ErrSessionExpired means the server accepted the login but rejected the
	// saved proactive-message context.
	ErrSessionExpired = errors.New("clawbot: 主动推送会话已失效，请先给 ClawBot 发送一条消息")
)

// Client is a minimal iLink ClawBot API client.
type Client struct {
	baseURL      string
	botToken     string
	botID        string
	userID       string
	contextToken string
	contextUser  string
	httpClient   *http.Client
	attempts     int
}

type httpStatusError struct {
	status int
	body   string
}

func (e *httpStatusError) Error() string {
	return fmt.Sprintf("clawbot: HTTP %d: %s", e.status, strings.TrimSpace(e.body))
}

type apiError struct {
	operation string
	ret       int
	errCode   int
	errMsg    string
}

func (e *apiError) Error() string {
	return fmt.Sprintf("clawbot: %s failed: ret=%d errcode=%d errmsg=%s", e.operation, e.ret, e.errCode, e.errMsg)
}

// NewClient creates a client from saved credentials.
func NewClient(creds Credentials) (*Client, error) {
	if err := validateCredentials(creds); err != nil {
		return nil, fmt.Errorf("clawbot: %w", err)
	}
	if strings.TrimSpace(creds.StaleAt) != "" {
		return nil, ErrStaleToken
	}
	return newClientWithBaseURL(creds, creds.BaseURL), nil
}

func newClientWithBaseURL(creds Credentials, baseURL string) *Client {
	baseURL = strings.TrimRight(strings.TrimSpace(baseURL), "/")
	if baseURL == "" {
		baseURL = DefaultBaseURL
	}
	return &Client{
		baseURL:      baseURL,
		botToken:     creds.BotToken,
		botID:        creds.ILinkBotID,
		userID:       creds.ILinkUserID,
		contextToken: creds.ContextToken,
		contextUser:  creds.ContextUserID,
		httpClient:   &http.Client{Timeout: defaultLongPollTimeout},
		attempts:     defaultSendAttempts,
	}
}

// SendText sends one plain-text message and returns the identifiers needed to
// correlate a later quoted reply.
func (c *Client) SendText(ctx context.Context, text string) (SendResult, error) {
	if strings.TrimSpace(text) == "" {
		return SendResult{}, fmt.Errorf("clawbot: message text is empty")
	}
	if strings.TrimSpace(c.contextToken) == "" ||
		strings.TrimSpace(c.contextUser) != strings.TrimSpace(c.userID) {
		return SendResult{}, ErrNoSession
	}

	clientID := randomClientID()
	accountScope := AccountScope(c.botID, c.userID)
	writeClawbotDebugEvent(DebugOperationSendRequest, DebugSendRequest{
		ClientID:     clientID,
		AccountScope: accountScope,
	})
	payload := sendMessageRequest{
		Msg: sendMessage{
			FromUserID:   "",
			ToUserID:     c.userID,
			ClientID:     clientID,
			MessageType:  MessageTypeBot,
			MessageState: MessageStateFinish,
			ContextToken: c.contextToken,
			ItemList: []messageItem{{
				Type:     ItemTypeText,
				TextItem: &textItem{Text: text},
			}},
		},
		BaseInfo: newBaseInfo(),
	}

	attempts := c.attempts
	if attempts <= 0 {
		attempts = defaultSendAttempts
	}
	delay := initialRetryDelay
	var lastErr error
	for attempt := 1; attempt <= attempts; attempt++ {
		attemptCtx, cancel := context.WithTimeout(ctx, sendTimeout)
		result, err := c.sendOnce(attemptCtx, payload)
		cancel()
		if err == nil {
			return result, nil
		}
		lastErr = err
		if attempt == attempts || !isRetryable(err) {
			break
		}

		timer := time.NewTimer(delay)
		select {
		case <-ctx.Done():
			timer.Stop()
			return SendResult{}, ctx.Err()
		case <-timer.C:
		}
		delay *= 2
	}
	return SendResult{}, lastErr
}

func (c *Client) sendOnce(ctx context.Context, payload sendMessageRequest) (SendResult, error) {
	var resp sendMessageResponse
	if err := c.postJSON(ctx, endpointSendMessage, payload, &resp); err != nil {
		return SendResult{}, err
	}
	if isSessionPreparationFailure(resp.Ret, resp.ErrCode, resp.ErrMsg) {
		businessErr := checkAPIStatus("sendmessage", resp.Ret, resp.ErrCode, resp.ErrMsg)
		return SendResult{}, fmt.Errorf("%w: %v", ErrSessionExpired, businessErr)
	}
	if err := checkAPIStatus("sendmessage", resp.Ret, resp.ErrCode, resp.ErrMsg); err != nil {
		return SendResult{}, err
	}
	result := resp.sendResult(payload.Msg.ClientID)
	writeClawbotDebugEvent(DebugOperationSendResult, DebugSendResult{
		MessageID:    result.MessageID,
		ClientID:     result.ClientID,
		AccountScope: AccountScope(c.botID, c.userID),
	})
	return result, nil
}

func isSessionPreparationFailure(ret, errCode int, errMsg string) bool {
	if ret != -2 && errCode != -2 {
		return false
	}
	return strings.Contains(strings.ToLower(strings.TrimSpace(errMsg)), "prepare failed")
}

// GetUpdates long-polls one batch of inbound messages. The returned cursor is
// the value that must be persisted for the next request.
func (c *Client) GetUpdates(ctx context.Context, cursor string) (Updates, error) {
	payload := getUpdatesRequest{
		GetUpdatesBuf: strings.TrimSpace(cursor),
		BaseInfo:      newBaseInfo(),
	}

	var resp getUpdatesResponse
	pollCtx, cancel := context.WithTimeout(ctx, defaultLongPollTimeout)
	defer cancel()
	if err := c.postJSON(pollCtx, endpointGetUpdates, payload, &resp); err != nil {
		return Updates{}, err
	}
	if err := checkAPIStatus("getupdates", resp.Ret, resp.ErrCode, resp.ErrMsg); err != nil {
		return Updates{}, err
	}

	result := Updates{
		Messages:           resp.Msgs,
		Cursor:             strings.TrimSpace(resp.GetUpdatesBuf),
		LongPollingTimeout: defaultLongPollTimeout,
	}
	if result.Cursor == "" {
		result.Cursor = strings.TrimSpace(resp.SyncBuf)
	}
	if resp.LongPollingTimeoutMS > 0 {
		result.LongPollingTimeout = time.Duration(resp.LongPollingTimeoutMS) * time.Millisecond
	}
	c.writeClawbotGetUpdatesDebug(result.Messages)
	return result, nil
}

func (c *Client) writeClawbotGetUpdatesDebug(messages []InboundMessage) {
	accountScope := AccountScope(c.botID, c.userID)
	records := make([]DebugInboundReference, 0, len(messages))
	for _, message := range messages {
		records = append(records, DebugInboundReference{
			MessageID:            message.PlatformMessageID(),
			HasReference:         message.HasReference(),
			ReferencedMessageIDs: message.ReferencedMessageIDs(),
			AccountScope:         accountScope,
			Private:              message.MessageType == MessageTypeUser && strings.TrimSpace(message.GroupID) == "",
			BoundSender:          strings.TrimSpace(message.FromUserID) == strings.TrimSpace(c.userID),
		})
	}
	writeClawbotDebugEvent(DebugOperationGetUpdatesData, records)
}

// NotifyStart announces that this client is online. Callers treat it as best
// effort and must not block message processing on failure.
func (c *Client) NotifyStart(ctx context.Context) error {
	return c.lifecycle(ctx, "/ilink/bot/msg/notifystart")
}

// NotifyStop announces that this client is going offline.
func (c *Client) NotifyStop(ctx context.Context) error {
	return c.lifecycle(ctx, "/ilink/bot/msg/notifystop")
}

func (c *Client) lifecycle(ctx context.Context, path string) error {
	var resp lifecycleResponse
	if err := c.postJSON(ctx, path, lifecycleRequest{BaseInfo: newBaseInfo()}, &resp); err != nil {
		return err
	}
	return checkAPIStatus(strings.TrimPrefix(path, "/ilink/bot/msg/"), resp.Ret, resp.ErrCode, resp.ErrMsg)
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
	req.Header.Set("Accept", "application/json")
	req.Header.Set("AuthorizationType", "ilink_bot_token")
	req.Header.Set("Authorization", "Bearer "+c.botToken)
	req.Header.Set("X-WECHAT-UIN", randomWechatUIN())
	req.Header.Set("iLink-App-Id", AppID)
	req.Header.Set("iLink-App-ClientVersion", AppClientVersion)

	resp, err := c.httpClient.Do(req)
	if err != nil {
		return fmt.Errorf("clawbot: http request: %w", err)
	}
	defer resp.Body.Close()

	respData, err := readResponseBody(resp.Body)
	if err != nil {
		return fmt.Errorf("clawbot: read response: %w", err)
	}
	if resp.StatusCode < 200 || resp.StatusCode >= 300 {
		return &httpStatusError{status: resp.StatusCode, body: string(respData)}
	}
	switch path {
	case endpointSendMessage:
		writeClawbotDebug(DebugOperationSendResponse, respData)
	case endpointGetUpdates:
		writeClawbotDebug(DebugOperationGetUpdates, respData)
	}
	if err := json.Unmarshal(respData, result); err != nil {
		return fmt.Errorf("clawbot: decode response: %w", err)
	}
	return nil
}

func checkAPIStatus(operation string, ret, errCode int, errMsg string) error {
	if ret == 0 && errCode == 0 {
		return nil
	}
	if ret == -14 || errCode == -14 {
		return fmt.Errorf("%w", ErrStaleToken)
	}
	return &apiError{operation: operation, ret: ret, errCode: errCode, errMsg: errMsg}
}

func isRetryable(err error) bool {
	if err == nil {
		return false
	}
	if errors.Is(err, ErrStaleToken) || errors.Is(err, ErrNoSession) {
		return false
	}
	if errors.Is(err, ErrSessionExpired) {
		return false
	}
	if errors.Is(err, context.Canceled) || errors.Is(err, context.DeadlineExceeded) {
		return false
	}
	var businessErr *apiError
	if errors.As(err, &businessErr) {
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
	var b [clientIDBytes]byte
	_, _ = rand.Read(b[:])
	return "agent-notify-" + hex.EncodeToString(b[:])
}

func randomWechatUIN() string {
	var n uint32
	if err := binary.Read(rand.Reader, binary.LittleEndian, &n); err != nil {
		n = uint32(time.Now().UnixNano())
	}
	return base64.StdEncoding.EncodeToString([]byte(fmt.Sprintf("%d", n)))
}
