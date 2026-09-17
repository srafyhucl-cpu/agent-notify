package clawbot

import (
	"bytes"
	"encoding/json"
	"fmt"
	"strings"
	"time"
)

// FlexibleString accepts either a JSON string or number for protocol fields
// whose type has varied between iLink releases.
type FlexibleString string

func (s *FlexibleString) UnmarshalJSON(data []byte) error {
	raw := strings.TrimSpace(string(data))
	if raw == "" || raw == "null" {
		*s = ""
		return nil
	}
	decoder := json.NewDecoder(bytes.NewReader(data))
	decoder.UseNumber()
	var value any
	if err := decoder.Decode(&value); err != nil {
		return err
	}
	switch typed := value.(type) {
	case string:
		*s = FlexibleString(strings.TrimSpace(typed))
	case json.Number:
		*s = FlexibleString(typed.String())
	default:
		return fmt.Errorf("clawbot: expected string or number, got %T", value)
	}
	return nil
}

func (s FlexibleString) String() string { return string(s) }

const (
	DefaultBaseURL = "https://ilinkai.weixin.qq.com"

	initialReferencedMessageIDCapacity = 4

	// ChannelVersion is the iLink protocol generation implemented by this client.
	ChannelVersion = "2.4.6"
	// AppID and AppClientVersion identify Agent-notify to the iLink CGI layer.
	AppID            = "bot"
	AppClientVersion = "132102"
	// BotAgent is attribution metadata carried in every business request.
	BotAgent = "Agent-notify/1.12.0 (windows)"

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
	MsgID    FlexibleString `json:"msg_id,omitempty"`
	Type     int            `json:"type"`
	TextItem *textItem      `json:"text_item,omitempty"`
	RefMsg   *refMessage    `json:"ref_msg,omitempty"`
}

type refMessage struct {
	MessageItem     *messageItem   `json:"message_item,omitempty"`
	MsgID           FlexibleString `json:"msg_id,omitempty"`
	ReferencedMsgID FlexibleString `json:"referenced_msg_id,omitempty"`
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
	Ret       int                 `json:"ret"`
	ErrCode   int                 `json:"errcode,omitempty"`
	ErrMsg    string              `json:"errmsg,omitempty"`
	MessageID FlexibleString      `json:"message_id,omitempty"`
	MsgID     FlexibleString      `json:"msg_id,omitempty"`
	MsgIDAlt  FlexibleString      `json:"msgid,omitempty"`
	ClientID  FlexibleString      `json:"client_id,omitempty"`
	Msg       sendMessageIDFields `json:"msg,omitempty"`
	Data      sendMessageIDFields `json:"data,omitempty"`
	ItemList  []messageItem       `json:"item_list,omitempty"`
}

type sendMessageIDFields struct {
	MessageID FlexibleString `json:"message_id,omitempty"`
	MsgID     FlexibleString `json:"msg_id,omitempty"`
	MsgIDAlt  FlexibleString `json:"msgid,omitempty"`
	ClientID  FlexibleString `json:"client_id,omitempty"`
	ItemList  []messageItem  `json:"item_list,omitempty"`
}

func (f *sendMessageIDFields) UnmarshalJSON(data []byte) error {
	raw := strings.TrimSpace(string(data))
	if raw == "" || raw == "null" {
		*f = sendMessageIDFields{}
		return nil
	}
	if strings.HasPrefix(raw, "{") {
		type fields sendMessageIDFields
		var value fields
		if err := json.Unmarshal(data, &value); err != nil {
			return err
		}
		*f = sendMessageIDFields(value)
		return nil
	}
	if strings.HasPrefix(raw, "[") {
		var items []messageItem
		if err := json.Unmarshal(data, &items); err != nil {
			return err
		}
		f.ItemList = items
		return nil
	}
	var value FlexibleString
	if err := json.Unmarshal(data, &value); err != nil {
		return err
	}
	f.MessageID = value
	return nil
}

// SendResult identifies one accepted outbound message.
type SendResult struct {
	MessageID string
	ClientID  string
}

func (r sendMessageResponse) sendResult(fallbackClientID string) SendResult {
	result := SendResult{
		MessageID: firstFlexibleString(r.MessageID, r.MsgID, r.MsgIDAlt),
		ClientID:  strings.TrimSpace(fallbackClientID),
	}
	if result.MessageID == "" {
		result.MessageID = firstMessageItemID(r.ItemList)
	}
	for _, nested := range []sendMessageIDFields{r.Msg, r.Data} {
		if result.MessageID == "" {
			result.MessageID = nested.messageID()
		}
		if result.MessageID != "" {
			break
		}
	}
	if result.ClientID == "" {
		result.ClientID = firstFlexibleString(r.ClientID, r.Msg.ClientID, r.Data.ClientID)
	}
	return result
}

func (f sendMessageIDFields) messageID() string {
	if value := firstFlexibleString(f.MessageID, f.MsgID, f.MsgIDAlt); value != "" {
		return value
	}
	return firstMessageItemID(f.ItemList)
}

func firstFlexibleString(values ...FlexibleString) string {
	for _, value := range values {
		if candidate := strings.TrimSpace(value.String()); candidate != "" {
			return candidate
		}
	}
	return ""
}

func firstMessageItemID(items []messageItem) string {
	for _, item := range items {
		if value := strings.TrimSpace(item.MsgID.String()); value != "" {
			return value
		}
	}
	return ""
}

// InboundMessage is one message returned by the getupdates long poll.
type InboundMessage struct {
	Seq             int64          `json:"seq,omitempty"`
	MsgID           FlexibleString `json:"msg_id,omitempty"`
	MessageID       FlexibleString `json:"message_id,omitempty"`
	FromUserID      string         `json:"from_user_id"`
	ToUserID        string         `json:"to_user_id"`
	MessageType     int            `json:"message_type"`
	MessageState    int            `json:"message_state,omitempty"`
	ContextToken    string         `json:"context_token,omitempty"`
	GroupID         string         `json:"group_id,omitempty"`
	ItemList        []messageItem  `json:"item_list,omitempty"`
	ReferencedMsgID FlexibleString `json:"referenced_msg_id,omitempty"`
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

// PlatformMessageID returns the inbound message identifier when the server
// provides one.
func (m InboundMessage) PlatformMessageID() string {
	for _, candidate := range []FlexibleString{m.MsgID, m.MessageID} {
		if value := strings.TrimSpace(candidate.String()); value != "" {
			return value
		}
	}
	for _, item := range m.ItemList {
		if value := strings.TrimSpace(item.MsgID.String()); value != "" {
			return value
		}
	}
	return ""
}

// HasReference reports whether the message shape contains a quoted message.
func (m InboundMessage) HasReference() bool {
	if strings.TrimSpace(m.ReferencedMsgID.String()) != "" {
		return true
	}
	for _, item := range m.ItemList {
		if item.RefMsg != nil {
			return true
		}
	}
	return false
}

// ReferencedMessageID returns the platform ID of the quoted outbound message.
func (m InboundMessage) ReferencedMessageID() string {
	ids := m.ReferencedMessageIDs()
	if len(ids) == 1 {
		return ids[0]
	}
	return ""
}

// ReferencedMessageIDs returns every distinct quoted-message ID exposed by
// the protocol. More than one value means the routing target is ambiguous.
func (m InboundMessage) ReferencedMessageIDs() []string {
	values := make([]string, 0, initialReferencedMessageIDCapacity)
	seen := make(map[string]struct{}, initialReferencedMessageIDCapacity)
	add := func(raw FlexibleString) {
		value := strings.TrimSpace(raw.String())
		if value == "" {
			return
		}
		if _, ok := seen[value]; ok {
			return
		}
		seen[value] = struct{}{}
		values = append(values, value)
	}

	add(m.ReferencedMsgID)
	for _, item := range m.ItemList {
		if item.RefMsg == nil {
			continue
		}
		if item.RefMsg.MessageItem != nil {
			add(item.RefMsg.MessageItem.MsgID)
		}
		add(item.RefMsg.MsgID)
		add(item.RefMsg.ReferencedMsgID)
	}
	return values
}

// ReferencedText returns the quoted text when the server includes it. Message
// routing must never depend on this value.
func (m InboundMessage) ReferencedText() string {
	for _, item := range m.ItemList {
		if item.RefMsg == nil || item.RefMsg.MessageItem == nil || item.RefMsg.MessageItem.TextItem == nil {
			continue
		}
		return item.RefMsg.MessageItem.TextItem.Text
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
