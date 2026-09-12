package clawbot

const (
	DefaultBaseURL = "https://ilinkai.weixin.qq.com"

	MessageTypeBot = 2

	MessageStateFinish = 2

	ItemTypeText = 1
)

// QRCodeResponse is returned while starting a new ClawBot login.
type QRCodeResponse struct {
	QRCode           string `json:"qrcode"`
	QRCodeImgContent string `json:"qrcode_img_content"`
	Ret              int    `json:"ret"`
}

// QRStatusResponse describes the current scan state.
type QRStatusResponse struct {
	Status      string `json:"status"`
	BotToken    string `json:"bot_token"`
	ILinkBotID  string `json:"ilink_bot_id"`
	BaseURL     string `json:"baseurl"`
	ILinkUserID string `json:"ilink_user_id"`
	Ret         int    `json:"ret"`
	ErrMsg      string `json:"errmsg,omitempty"`
}

// Credentials contains the secret used to send proactive ClawBot messages.
type Credentials struct {
	BotToken    string `json:"bot_token"`
	ILinkBotID  string `json:"ilink_bot_id"`
	BaseURL     string `json:"baseurl,omitempty"`
	ILinkUserID string `json:"ilink_user_id"`
}

// Status summarizes whether ClawBot credentials are available without exposing secrets.
type Status struct {
	LoggedIn   bool   `json:"loggedIn"`
	Path       string `json:"path"`
	BaseURL    string `json:"baseURL,omitempty"`
	ILinkBotID string `json:"ilinkBotID,omitempty"`
	UserHint   string `json:"userHint,omitempty"`
}

type baseInfo struct {
	ChannelVersion string `json:"channel_version,omitempty"`
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
	Ret    int    `json:"ret"`
	ErrMsg string `json:"errmsg,omitempty"`
}
