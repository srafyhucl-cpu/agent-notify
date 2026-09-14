package reply

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"strings"
	"time"
	"unicode/utf8"

	"github.com/srafyhucl-cpu/agent-notify/internal/agentmeta"
	"github.com/srafyhucl-cpu/agent-notify/internal/clawbot"
	"github.com/srafyhucl-cpu/agent-notify/internal/config"
)

const (
	maxFailureRunes = 300
	maxReplyRunes   = 8000
	dispatchWait    = 40 * time.Second
	errorSendWait   = 15 * time.Second
)

// TextSender sends one visible error message back to the bound WeChat chat.
type TextSender func(ctx context.Context, text string) error

type dedupAccountScope struct {
	BotID  string `json:"botID"`
	UserID string `json:"userID"`
}

type dedupFallbackFields struct {
	Seq          int64  `json:"seq"`
	FromUserID   string `json:"fromUserID"`
	ReferencedID string `json:"referencedID"`
	Text         string `json:"text"`
}

// DispatcherOptions supplies the persisted state and per-Agent senders used
// by a Dispatcher. Nil dependencies use production defaults.
type DispatcherOptions struct {
	Routes   *RouteStore
	State    *StateStore
	Load     func() (config.AppConfig, error)
	Senders  map[string]ReplySender
	SendText TextSender
	Now      func() time.Time
}

// Dispatcher routes quoted ClawBot replies to the exact outbound Agent session.
type Dispatcher struct {
	routes   *RouteStore
	state    *StateStore
	load     func() (config.AppConfig, error)
	senders  map[string]ReplySender
	sendText TextSender
	now      func() time.Time
}

// NewDispatcher builds a dispatcher with all supported agent senders available
// by default. Callers may replace a sender or register additional agents.
func NewDispatcher(options DispatcherOptions) *Dispatcher {
	if options.Routes == nil {
		options.Routes = NewRouteStore("")
	}
	if options.State == nil {
		options.State = NewStateStore("")
	}
	if options.Load == nil {
		options.Load = func() (config.AppConfig, error) {
			return config.LoadConfig("")
		}
	}
	if options.SendText == nil {
		options.SendText = NewClawBotTextSender()
	}
	if options.Now == nil {
		options.Now = time.Now
	}
	dispatcher := &Dispatcher{
		routes:   options.Routes,
		state:    options.State,
		load:     options.Load,
		senders:  make(map[string]ReplySender),
		sendText: options.SendText,
		now:      options.Now,
	}
	for agent, sender := range options.Senders {
		agent = strings.ToLower(strings.TrimSpace(agent))
		if agent != "" && sender != nil {
			dispatcher.senders[agent] = sender
		}
	}
	if _, ok := dispatcher.senders["codex"]; !ok {
		dispatcher.senders["codex"] = CodexReplySender{Queue: CodexQueueRunner{}}
	}
	if _, ok := dispatcher.senders["opencode"]; !ok {
		dispatcher.senders["opencode"] = OpenCodeReplySender{Queue: OpenCodeQueueRunner{
			OnAsyncFailure: func(_ string, _ string, err error) {
				dispatcher.fail("", fmt.Sprintf("发送到 OpenCode 失败：%s", compactError(err)))
			},
		}}
	}
	if _, ok := dispatcher.senders[agentmeta.Antigravity]; !ok {
		dispatcher.senders[agentmeta.Antigravity] = AntigravityAgentAPISender{}
	}
	if _, ok := dispatcher.senders[agentmeta.Devin]; !ok {
		dispatcher.senders[agentmeta.Devin] = DevinReplySender{Queue: DevinQueueRunner{
			OnAsyncFailure: func(_ string, _ string, err error) {
				dispatcher.fail("", fmt.Sprintf("发送到 Devin 失败：%s", compactError(err)))
			},
		}}
	}
	return dispatcher
}

// Handle processes one inbound message. Ordinary messages return without side
// effects; quoted messages are claimed before an Agent command is launched.
func (d *Dispatcher) Handle(message clawbot.InboundMessage) {
	if !message.HasReference() {
		return
	}

	cfg, err := d.load()
	if err != nil {
		d.logf("load config: %v", err)
		return
	}
	if !cfg.ReplyEnabled {
		return
	}

	// Reject non-private or non-bound senders before any visible error can
	// be sent back to the bound chat.
	credentials, err := clawbot.LoadCredentials()
	if err != nil {
		d.logf("load credentials: %v", err)
		return
	}
	if message.MessageType != clawbot.MessageTypeUser ||
		strings.TrimSpace(message.GroupID) != "" ||
		strings.TrimSpace(message.FromUserID) != strings.TrimSpace(credentials.ILinkUserID) {
		return
	}

	referencedIDs := message.ReferencedMessageIDs()
	text := strings.TrimSpace(message.Text())
	key := inboundDedupKey(credentials.ILinkBotID, credentials.ILinkUserID, message, strings.Join(referencedIDs, "\x00"), text)
	claimed, err := d.state.Claim(key)
	if err != nil {
		d.fail("", "无法续聊：本地去重状态不可用，已停止转发以避免重复执行。")
		d.logf("claim %s: %v", key, err)
		return
	}
	if !claimed {
		return
	}

	referencedID, failure := validateReplyPayload(referencedIDs, text)
	if failure != "" {
		d.fail(key, failure)
		return
	}

	route, err := d.routes.Find(credentials.ILinkBotID, credentials.ILinkUserID, referencedID, referencedID)
	// Protocol revisions have exposed either the platform message ID or the
	// client ID in the quote. RouteStore treats both as exact identifiers and
	// rejects a cross-field collision instead of guessing.
	if err != nil {
		switch {
		case errors.Is(err, ErrRouteNotFound), errors.Is(err, ErrRouteExpired):
			d.fail(key, fmt.Sprintf("无法续聊：这条通知没有可用的会话记录，可能已超过 %s 或未建立引用关联。", routeExpiryLabel()))
		case errors.Is(err, ErrRouteAmbiguous):
			d.fail(key, "无法续聊：引用消息匹配到多个会话，已停止转发以避免误发。")
		default:
			d.fail(key, "无法续聊：读取会话路由失败。")
			d.logf("find route: %v", err)
		}
		return
	}

	ctx, cancel := context.WithTimeout(context.Background(), dispatchWait)
	defer cancel()
	if err := d.dispatch(ctx, route, text); err != nil {
		d.fail(key, dispatchFailureMessage(route.Agent, err))
		return
	}
	// Success is recorded without the reply text so acceptance and
	// troubleshooting can prove the target session without storing content.
	d.logf("dispatched quoted=%s agent=%s session=%s", referencedID, route.Agent, route.SessionID)
	if err := d.state.Mark(key, replyStateSent); err != nil {
		d.logf("mark sent %s: %v", key, err)
	}
}

func (d *Dispatcher) dispatch(ctx context.Context, route Route, text string) error {
	agent := strings.ToLower(strings.TrimSpace(route.Agent))
	sender := d.senders[agent]
	if sender == nil {
		return fmt.Errorf("暂不支持续聊 Agent %q", route.Agent)
	}
	return sender.Send(ctx, route.SessionID, text)
}

func dispatchFailureMessage(agent string, err error) string {
	label := agentLabel(agent)
	detail := compactError(err)
	if errors.Is(err, ErrCodexQueueUnconfirmed) {
		return fmt.Sprintf("发送到 %s 的结果未确认：%s；系统未自动重试，请先在对应会话中确认，避免重复发送。", label, detail)
	}
	return fmt.Sprintf("发送到 %s 失败：%s", label, detail)
}

func (d *Dispatcher) fail(key, message string) {
	message = strings.TrimSpace(message)
	if message == "" || d.sendText == nil {
		return
	}
	// Record why the reply was rejected before the visible notice is sent,
	// so a failed send still leaves a local trail for troubleshooting.
	d.logf("visible error: %s", message)
	ctx, cancel := context.WithTimeout(context.Background(), errorSendWait)
	defer cancel()
	if err := d.sendText(ctx, message); err != nil {
		d.logf("send failure notice: %v", err)
	}
	if key != "" {
		_ = d.state.Mark(key, replyStateFailed)
	}
}

func inboundDedupKey(botID, userID string, message clawbot.InboundMessage, referencedID, text string) string {
	scopeRaw, _ := json.Marshal(dedupAccountScope{
		BotID:  strings.TrimSpace(botID),
		UserID: strings.TrimSpace(userID),
	})
	scope := sha256.Sum256(scopeRaw)
	prefix := "account:" + hex.EncodeToString(scope[:]) + ":"
	if id := strings.TrimSpace(message.PlatformMessageID()); id != "" {
		return prefix + "message:" + id
	}
	raw, _ := json.Marshal(dedupFallbackFields{
		Seq:          message.Seq,
		FromUserID:   strings.TrimSpace(message.FromUserID),
		ReferencedID: strings.TrimSpace(referencedID),
		Text:         text,
	})
	sum := sha256.Sum256(raw)
	return prefix + "fallback:" + hex.EncodeToString(sum[:])
}

// validateReplyPayload returns the single quoted message ID plus the failure
// text for one inbound reply. A non-empty failure is safe to send back to the
// bound chat and means the reply must not be dispatched.
func validateReplyPayload(referencedIDs []string, text string) (string, string) {
	switch {
	case len(referencedIDs) == 0:
		return "", "无法续聊：微信引用消息中没有可用的消息 ID。请更新 ClawBot 协议字段后重试。"
	case len(referencedIDs) > 1:
		return "", "无法续聊：引用消息包含多个不一致的消息 ID，已停止转发以避免误发。"
	case strings.TrimSpace(text) == "":
		return "", "无法续聊：引用回复内容为空。"
	case utf8.RuneCountInString(text) > maxReplyRunes:
		return "", fmt.Sprintf("无法续聊：回复超过 %d 个字符。", maxReplyRunes)
	}
	return strings.TrimSpace(referencedIDs[0]), ""
}

func agentLabel(agent string) string {
	if descriptor, ok := agentmeta.Lookup(agent); ok {
		return descriptor.DisplayName
	}
	return strings.TrimSpace(agent)
}

func compactError(err error) string {
	if err == nil {
		return ""
	}
	return truncateRunes(strings.Join(strings.Fields(err.Error()), " "), maxFailureRunes)
}

func routeExpiryLabel() string {
	days := int(DefaultRouteTTL / (24 * time.Hour))
	return fmt.Sprintf("%d 天", days)
}

func (d *Dispatcher) logf(format string, args ...any) {
	writeReplyDiagnosticAt(d.now(), format, args...)
}
