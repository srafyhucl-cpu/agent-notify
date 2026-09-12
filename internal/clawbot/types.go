package clawbot

import (
	"strings"
	"time"
)

const (
	DefaultBaseURL = "https://ilinkai.weixin.qq.com"

	// ChannelVersion is the iLink protocol generation implemented by this client.
	ChannelVersion = "2.4.6"
	// AppID and AppClientVersion identify Agent-notify to the iLink CGI layer.
	AppID            = "bot"
	AppClientVersion = "132102"
	// BotAgent is attribution metadata carried in every business request.
	BotAgent = "Agent-notify/1.0.3 (windows)"

	MessageTypeBot = 2

	MessageTypeUser = 1

	MessageStateFinish = 2

	ItemTypeText = 1

	defaultLongPollTimeout = 35 * time.Second
)

// QRCodeResponse is returned while starting a new ClawBot login.
type QRCodeResponse struct {
	QRCode           string `json:"qrcode"`
	QRCodeImgContent string `json:"qrcode_img_content"`
	Ret              int    `json:"ret"`
	ErrCode          int    `json:"errcode,omitempty"`
	ErrMsg           string `json:"errmsg,omitempty"`
}

// DisplayContent returns the payload to encode into the QR image. The official
// client renders qrcode_img_content when present and falls back to qrcode.
func (r QRCodeResponse) DisplayContent() string {
	if content := strings.TrimSpace(r.QRCodeImgContent); content != "" {
		return content
	}
	return strings.TrimSpace(r.QRCode)
}

// QRStatusResponse describes the current scan state.
type QRStatusResponse struct {
	Status            string `json:"status"`
	BotToken          string `json:"bot_token"`
	ILinkBotID        string `json:"ilink_bot_id"`
	BaseURL           string `json:"baseurl"`
	ILinkUserID       string `json:"ilink_user_id"`
	RedirectHost      string `json:"redirect_host,omitempty"`
	BindedRedirect    bool   `json:"binded_redirect,omitempty"`
	NeedVerifyCode    bool   `json:"need_verifycode,omitempty"`
	VerifyCodeBlocked bool   `json:"verify_code_blocked,omitempty"`
	Ret               int    `json:"ret"`
	ErrCode           int    `json:"errcode,omitempty"`
	ErrMsg            string `json:"errmsg,omitempty"`
}

// Credentials contains the secret used to send proactive ClawBot messages.
type Credentials struct {
	BotToken      string `json:"bot_token"`
	ILinkBotID    string `json:"ilink_bot_id"`
	BaseURL       string `json:"baseurl,omitempty"`
	ILinkUserID   string `json:"ilink_user_id"`
	ContextToken  string `json:"context_token,omitempty"`
	ContextUserID string `json:"context_user_id,omitempty"`
	GetUpdatesBuf string `json:"get_updates_buf,omitempty"`
	StaleAt       string `json:"stale_at,omitempty"`
}

// Status summarizes whether ClawBot credentials are available without exposing secrets.
type Status struct {
	LoggedIn     bool   `json:"loggedIn"`
	SessionReady bool   `json:"sessionReady"`
	Stale        bool   `json:"stale,omitempty"`
	Path         string `json:"path"`
	BaseURL      string `json:"baseURL,omitempty"`
	ILinkBotID   string `json:"ilinkBotID,omitempty"`
	UserHint     string `json:"userHint,omitempty"`
}

type baseInfo struct {
	ChannelVersion string `json:"channel_version,omitempty"`
	BotAgent       string `json:"bot_agent,omitempty"`
}

type textItem struct {
	Text string `json:"text"`
}

type messageItem struct {
	Type     int       `json:"type"`
	TextItem *textItem `json:"text_item,omitempty"`
}

type sendMessage struct {
	FromUserID   string        `json:"from_user_id"`
	ToUserID     string        `json:"to_user_id"`
	ClientID     string        `json:"client_id"`
	MessageType  int           `json:"message_type"`
	MessageState int           `json:"message_state"`
	ItemList     []messageItem `json:"item_list"`
	ContextToken string        `json:"context_token,omitempty"`
}

type sendMessageRequest struct {
	Msg      sendMessage `json:"msg"`
	BaseInfo baseInfo    `json:"base_info"`
}

type sendMessageResponse struct {
	Ret     int    `json:"ret"`
	ErrCode int    `json:"errcode,omitempty"`
	ErrMsg  string `json:"errmsg,omitempty"`
}

// InboundMessage is one message returned by the getupdates long poll.
type InboundMessage struct {
	Seq          int64         `json:"seq,omitempty"`
	FromUserID   string        `json:"from_user_id"`
	ToUserID     string        `json:"to_user_id"`
	MessageType  int           `json:"message_type"`
	MessageState int           `json:"message_state,omitempty"`
	ContextToken string        `json:"context_token,omitempty"`
	GroupID      string        `json:"group_id,omitempty"`
	ItemList     []messageItem `json:"item_list,omitempty"`
}

// Text returns the first text item of an inbound message.
func (m InboundMessage) Text() string {
	for _, item := range m.ItemList {
		if item.Type == ItemTypeText && item.TextItem != nil {
			return item.TextItem.Text
		}
	}
	return ""
}

// Updates is one successful getupdates response.
type Updates struct {
	Messages           []InboundMessage
	Cursor             string
	LongPollingTimeout time.Duration
}

type qrCodeRequest struct {
	LocalTokenList []string `json:"local_token_list"`
	BaseInfo       baseInfo `json:"base_info"`
}

type getUpdatesRequest struct {
	GetUpdatesBuf string   `json:"get_updates_buf"`
	BaseInfo      baseInfo `json:"base_info"`
}

type getUpdatesResponse struct {
	Ret                  int              `json:"ret"`
	ErrCode              int              `json:"errcode,omitempty"`
	ErrMsg               string           `json:"errmsg,omitempty"`
	Msgs                 []InboundMessage `json:"msgs"`
	GetUpdatesBuf        string           `json:"get_updates_buf"`
	SyncBuf              string           `json:"sync_buf,omitempty"`
	LongPollingTimeoutMS int              `json:"longpolling_timeout_ms,omitempty"`
}

type lifecycleRequest struct {
	BaseInfo baseInfo `json:"base_info"`
}

type lifecycleResponse struct {
	Ret     int    `json:"ret"`
	ErrCode int    `json:"errcode,omitempty"`
	ErrMsg  string `json:"errmsg,omitempty"`
}

func newBaseInfo() baseInfo {
	return baseInfo{ChannelVersion: ChannelVersion, BotAgent: BotAgent}
}
