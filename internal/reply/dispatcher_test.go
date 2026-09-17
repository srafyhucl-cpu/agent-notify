package reply

import (
	"context"
	"encoding/json"
	"fmt"
	"os"
	"path/filepath"
	"strings"
	"sync"
	"testing"
	"time"

	"github.com/srafyhucl-cpu/agent-notify/internal/clawbot"
	"github.com/srafyhucl-cpu/agent-notify/internal/config"
)

type fakeCodexQueue struct {
	mu        sync.Mutex
	threadIDs []string
	messages  []string
	err       error
}

func (q *fakeCodexQueue) Queue(_ context.Context, threadID, text string) error {
	q.mu.Lock()
	defer q.mu.Unlock()
	q.threadIDs = append(q.threadIDs, threadID)
	q.messages = append(q.messages, text)
	return q.err
}

type fakeOpenCodeQueue struct {
	fakeCodexQueue
}

func quotedMessage(t *testing.T, messageID, referencedID, text string) clawbot.InboundMessage {
	t.Helper()
	raw := fmt.Sprintf(`{
		"msg_id": %q,
		"seq": 42,
		"from_user_id": "user-1",
		"message_type": 1,
		"context_token": "ctx",
		"item_list": [
			{"type": 1, "text_item": {"text": %q}},
			{"type": 3, "ref_msg": {"message_item": {"msg_id": %q}}}
		]
	}`, messageID, text, referencedID)
	var message clawbot.InboundMessage
	if err := json.Unmarshal([]byte(raw), &message); err != nil {
		t.Fatalf("decode test message: %v", err)
	}
	return message
}

func newTestDispatcher(t *testing.T, cfg config.AppConfig) (*Dispatcher, *fakeCodexQueue, *[]string) {
	t.Helper()
	dir := t.TempDir()
	t.Setenv("AGENT_NOTIFY_CONFIG_DIR", dir)
	t.Setenv("AGENT_NOTIFY_TEMP_DIR", filepath.Join(dir, "temp"))
	if err := clawbot.SaveCredentials(clawbot.Credentials{
		BotToken:      "token",
		ILinkBotID:    "bot-1",
		ILinkUserID:   "user-1",
		ContextToken:  "ctx",
		ContextUserID: "user-1",
	}); err != nil {
		t.Fatalf("SaveCredentials: %v", err)
	}

	routes := NewRouteStore(filepath.Join(dir, "routes.jsonl"))
	state := NewStateStore(filepath.Join(dir, "state.jsonl"))
	queue := &fakeCodexQueue{}
	var failures []string
	sender := func(_ context.Context, text string) error {
		failures = append(failures, text)
		return nil
	}
	dispatcher := NewDispatcher(DispatcherOptions{
		Routes: routes,
		State:  state,
		Load:   func() (config.AppConfig, error) { return cfg, nil },
		Senders: map[string]ReplySender{
			"codex": CodexReplySender{Queue: queue},
		},
		SendText: sender,
	})
	return dispatcher, queue, &failures
}

func TestDispatcherRoutesExactCodexThread(t *testing.T) {
	dispatcher, queue, failures := newTestDispatcher(t, config.AppConfig{ReplyEnabled: true})
	if err := dispatcher.routes.Record(Route{
		MessageID: "platform-1",
		BotID:     "bot-1",
		UserID:    "user-1",
		Agent:     "codex",
		SessionID: "thread-1",
	}); err != nil {
		t.Fatal(err)
	}

	dispatcher.Handle(quotedMessage(t, "reply-1", "platform-1", "  继续检查  "))
	if len(queue.threadIDs) != 1 || queue.threadIDs[0] != "thread-1" {
		t.Fatalf("thread IDs = %#v", queue.threadIDs)
	}
	if len(queue.messages) != 1 || queue.messages[0] != "继续检查" {
		t.Fatalf("messages = %#v", queue.messages)
	}
	if len(*failures) != 0 {
		t.Fatalf("unexpected failure notices: %#v", *failures)
	}

	dispatcher.Handle(quotedMessage(t, "reply-1", "platform-1", "继续检查"))
	if len(queue.threadIDs) != 1 {
		t.Fatalf("duplicate reply was dispatched: %#v", queue.threadIDs)
	}
}

func TestDispatcherRoutesObservedWeChatQuoteShape(t *testing.T) {
	dispatcher, queue, failures := newTestDispatcher(t, config.AppConfig{ReplyEnabled: true})
	const platformMessageID = "7504586663532869128"
	if err := dispatcher.routes.Record(Route{
		MessageID: platformMessageID,
		BotID:     "bot-1",
		UserID:    "user-1",
		Agent:     "codex",
		SessionID: "thread-1",
	}); err != nil {
		t.Fatal(err)
	}

	var message clawbot.InboundMessage
	raw := `{
		"client_id":"mmassistant_bypmsg_inbox_wxid_observed",
		"from_user_id":"user-1",
		"group_id":"",
		"item_list":[{
			"msg_id":"v1:3763981292873992349",
			"ref_msg":{"message_item":{"msg_id":7504586663532869128}},
			"text_item":{"text":"继续检查"},
			"type":1
		}],
		"message_id":7504586783419756808,
		"message_type":1,
		"seq":7
	}`
	if err := json.Unmarshal([]byte(raw), &message); err != nil {
		t.Fatal(err)
	}
	if got := message.PlatformMessageID(); got != "7504586783419756808" {
		t.Fatalf("PlatformMessageID = %q", got)
	}

	dispatcher.Handle(message)
	if len(queue.threadIDs) != 1 || queue.threadIDs[0] != "thread-1" {
		t.Fatalf("thread IDs = %#v", queue.threadIDs)
	}
	if len(*failures) != 0 {
		t.Fatalf("failure notices = %#v", *failures)
	}
}

func TestDispatcherMatchesReferencedClientIDExactly(t *testing.T) {
	dispatcher, queue, failures := newTestDispatcher(t, config.AppConfig{ReplyEnabled: true})
	if err := dispatcher.routes.Record(Route{
		ClientID:  "client-1",
		BotID:     "bot-1",
		UserID:    "user-1",
		Agent:     "codex",
		SessionID: "thread-1",
	}); err != nil {
		t.Fatal(err)
	}

	dispatcher.Handle(quotedMessage(t, "reply-client-id", "client-1", "继续"))
	if len(queue.threadIDs) != 1 || queue.threadIDs[0] != "thread-1" {
		t.Fatalf("thread IDs = %#v", queue.threadIDs)
	}
	if len(*failures) != 0 {
		t.Fatalf("unexpected failure notices: %#v", *failures)
	}
}

func TestDispatcherNeverFallsBackToAnotherRoute(t *testing.T) {
	dispatcher, queue, failures := newTestDispatcher(t, config.AppConfig{ReplyEnabled: true})
	if err := dispatcher.routes.Record(Route{
		MessageID: "platform-1",
		BotID:     "bot-1",
		UserID:    "user-1",
		Agent:     "codex",
		SessionID: "thread-1",
	}); err != nil {
		t.Fatal(err)
	}

	dispatcher.Handle(quotedMessage(t, "reply-2", "platform-2", "继续"))
	if len(queue.threadIDs) != 0 {
		t.Fatalf("unexpected dispatch: %#v", queue.threadIDs)
	}
	if len(*failures) != 1 || !strings.Contains((*failures)[0], "没有可用的会话记录") {
		t.Fatalf("failure notices = %#v", *failures)
	}
}

func TestDispatcherSkipsWhenDisabledOrOutsidePrivateBoundChat(t *testing.T) {
	dispatcher, queue, _ := newTestDispatcher(t, config.AppConfig{ReplyEnabled: false})
	if err := dispatcher.routes.Record(Route{
		MessageID: "platform-1",
		BotID:     "bot-1",
		UserID:    "user-1",
		Agent:     "codex",
		SessionID: "thread-1",
	}); err != nil {
		t.Fatal(err)
	}
	dispatcher.Handle(quotedMessage(t, "reply-disabled", "platform-1", "继续"))
	if len(queue.threadIDs) != 0 {
		t.Fatal("disabled dispatcher queued a message")
	}

	dispatcher, queue, failures := newTestDispatcher(t, config.AppConfig{ReplyEnabled: true})
	if err := dispatcher.routes.Record(Route{
		MessageID: "platform-1",
		BotID:     "bot-1",
		UserID:    "user-1",
		Agent:     "codex",
		SessionID: "thread-1",
	}); err != nil {
		t.Fatal(err)
	}
	group := quotedMessage(t, "reply-group", "platform-1", "继续")
	group.GroupID = "group-1"
	dispatcher.Handle(group)
	other := quotedMessage(t, "reply-other", "platform-1", "继续")
	other.FromUserID = "stranger"
	dispatcher.Handle(other)
	groupEmpty := quotedMessage(t, "reply-group-empty", "platform-1", "")
	groupEmpty.GroupID = "group-1"
	dispatcher.Handle(groupEmpty)
	otherEmpty := quotedMessage(t, "reply-other-empty", "platform-1", "")
	otherEmpty.FromUserID = "stranger"
	dispatcher.Handle(otherEmpty)
	if len(queue.threadIDs) != 0 || len(*failures) != 0 {
		t.Fatalf("non-private messages caused side effects: queue=%#v failures=%#v", queue.threadIDs, *failures)
	}
}

func TestDispatcherReportsMissingReferenceID(t *testing.T) {
	dispatcher, queue, failures := newTestDispatcher(t, config.AppConfig{ReplyEnabled: true})
	message := quotedMessage(t, "reply-1", "", "继续")
	dispatcher.Handle(message)
	dispatcher.Handle(message)
	if len(queue.threadIDs) != 0 {
		t.Fatalf("unexpected dispatch: %#v", queue.threadIDs)
	}
	if len(*failures) != 1 || !strings.Contains((*failures)[0], "消息 ID") {
		t.Fatalf("failure notices = %#v", *failures)
	}
}

func TestDispatcherReportsEmptyReplyText(t *testing.T) {
	dispatcher, queue, failures := newTestDispatcher(t, config.AppConfig{ReplyEnabled: true})
	dispatcher.Handle(quotedMessage(t, "reply-empty", "platform-1", "  \n  "))
	dispatcher.Handle(quotedMessage(t, "reply-empty", "platform-1", "  \n  "))
	if len(queue.threadIDs) != 0 {
		t.Fatalf("unexpected dispatch: %#v", queue.threadIDs)
	}
	if len(*failures) != 1 || !strings.Contains((*failures)[0], "内容为空") {
		t.Fatalf("failure notices = %#v", *failures)
	}
}

func TestDispatcherRejectsConflictingReferenceIDs(t *testing.T) {
	dispatcher, queue, failures := newTestDispatcher(t, config.AppConfig{ReplyEnabled: true})
	message := quotedMessage(t, "reply-conflict", "platform-1", "继续")
	message.ReferencedMsgID = "platform-2"
	dispatcher.Handle(message)
	dispatcher.Handle(message)
	if len(queue.threadIDs) != 0 {
		t.Fatalf("unexpected dispatch: %#v", queue.threadIDs)
	}
	if len(*failures) != 1 || !strings.Contains((*failures)[0], "多个不一致") {
		t.Fatalf("failure notices = %#v", *failures)
	}
}

func TestInboundDedupKeyFallsBackDeterministically(t *testing.T) {
	message := quotedMessage(t, "", "platform-1", "继续")
	first := inboundDedupKey("bot-1", "user-1", message, "platform-1", "继续")
	second := inboundDedupKey("bot-1", "user-1", message, "platform-1", "继续")
	if first != second || !strings.Contains(first, ":fallback:") || !strings.HasPrefix(first, "account:") {
		t.Fatalf("dedup keys = %q / %q", first, second)
	}
	message.Seq++
	if first == inboundDedupKey("bot-1", "user-1", message, "platform-1", "继续") {
		t.Fatal("different seq values produced the same fallback key")
	}
}

func TestInboundDedupKeyIsScopedToClawBotAccount(t *testing.T) {
	message := quotedMessage(t, "reply-1", "platform-1", "继续")
	first := inboundDedupKey("bot-1", "user-1", message, "platform-1", "继续")
	second := inboundDedupKey("bot-2", "user-1", message, "platform-1", "继续")
	if first == second {
		t.Fatalf("different ClawBot accounts produced the same dedup key: %q", first)
	}
}

func TestInboundDedupKeyFallbackKeepsFieldBoundaries(t *testing.T) {
	firstMessage := quotedMessage(t, "", "platform-1", "ignored")
	firstMessage.FromUserID = "a"
	secondMessage := quotedMessage(t, "", "platform-1", "ignored")
	secondMessage.FromUserID = "a\nb"

	first := inboundDedupKey("bot-1", "user-1", firstMessage, "b\nc", "d")
	second := inboundDedupKey("bot-1", "user-1", secondMessage, "c", "d")
	if first == second {
		t.Fatalf("field boundaries collided: %q", first)
	}
}

func TestDispatcherReportsCodexFailure(t *testing.T) {
	dispatcher, queue, failures := newTestDispatcher(t, config.AppConfig{ReplyEnabled: true})
	queue.err = fmt.Errorf("thread missing")
	if err := dispatcher.routes.Record(Route{
		MessageID: "platform-1",
		BotID:     "bot-1",
		UserID:    "user-1",
		Agent:     "codex",
		SessionID: "thread-1",
	}); err != nil {
		t.Fatal(err)
	}
	dispatcher.Handle(quotedMessage(t, "reply-1", "platform-1", "继续"))
	if len(*failures) != 1 || !strings.Contains((*failures)[0], "thread missing") {
		t.Fatalf("failure notices = %#v", *failures)
	}
	diagnostics := readReplyDiagnostics(t)
	if !strings.Contains(diagnostics, "visible error:") || !strings.Contains(diagnostics, "thread missing") {
		t.Fatalf("diagnostics = %q, want the visible dispatch error", diagnostics)
	}
	if strings.Contains(diagnostics, "继续") {
		t.Fatalf("diagnostics leaked reply text: %q", diagnostics)
	}
}

func TestDispatcherReportsUnconfirmedCodexDelivery(t *testing.T) {
	dispatcher, queue, failures := newTestDispatcher(t, config.AppConfig{ReplyEnabled: true})
	queue.err = fmt.Errorf("%w: 超时", ErrCodexQueueUnconfirmed)
	if err := dispatcher.routes.Record(Route{
		MessageID: "platform-1",
		BotID:     "bot-1",
		UserID:    "user-1",
		Agent:     "codex",
		SessionID: "thread-1",
	}); err != nil {
		t.Fatal(err)
	}
	dispatcher.Handle(quotedMessage(t, "reply-unconfirmed", "platform-1", "继续"))
	if len(*failures) != 1 || !strings.Contains((*failures)[0], "结果未确认") || !strings.Contains((*failures)[0], "未自动重试") {
		t.Fatalf("failure notices = %#v", *failures)
	}
}

func TestDispatcherWiresDefaultOpenCodeAsyncFailureReporter(t *testing.T) {
	dispatcher, _, failures := newTestDispatcher(t, config.AppConfig{ReplyEnabled: true})
	sender, ok := dispatcher.senders["opencode"].(OpenCodeReplySender)
	if !ok {
		t.Fatalf("default OpenCode sender = %#v", dispatcher.senders["opencode"])
	}
	runner, ok := sender.Queue.(OpenCodeQueueRunner)
	if !ok || runner.OnAsyncFailure == nil {
		t.Fatalf("default OpenCode runner = %#v, want async failure reporter", sender.Queue)
	}

	runner.OnAsyncFailure("session-1", "继续", fmt.Errorf("prompt failed"))
	if len(*failures) != 1 || !strings.Contains((*failures)[0], "OpenCode") || !strings.Contains((*failures)[0], "prompt failed") {
		t.Fatalf("failure notices = %#v", *failures)
	}
}

func TestDispatcherUsesConfiguredClockForState(t *testing.T) {
	dispatcher, _, _ := newTestDispatcher(t, config.AppConfig{ReplyEnabled: true})
	fixed := time.Date(2026, 9, 12, 10, 0, 0, 0, time.Local)
	dispatcher.now = func() time.Time { return fixed }
	if got := dispatcher.now(); !got.Equal(fixed) {
		t.Fatalf("clock = %v", got)
	}
}

func TestDispatcherStopsWhenDedupStateUnavailable(t *testing.T) {
	dispatcher, queue, failures := newTestDispatcher(t, config.AppConfig{ReplyEnabled: true})
	if err := dispatcher.routes.Record(Route{
		MessageID: "platform-1",
		BotID:     "bot-1",
		UserID:    "user-1",
		Agent:     "codex",
		SessionID: "thread-1",
	}); err != nil {
		t.Fatal(err)
	}
	dispatcher.state.Path = unavailableChildPath(t, "state.jsonl")

	dispatcher.Handle(quotedMessage(t, "reply-state-error", "platform-1", "继续"))
	if len(queue.threadIDs) != 0 {
		t.Fatalf("unsafe dispatch without dedup state: %#v", queue.threadIDs)
	}
	if len(*failures) != 1 || !strings.Contains((*failures)[0], "本地去重状态不可用") {
		t.Fatalf("failure notices = %#v", *failures)
	}
}

func TestDispatcherStopsWhenRouteStoreUnavailable(t *testing.T) {
	dispatcher, queue, failures := newTestDispatcher(t, config.AppConfig{ReplyEnabled: true})
	dispatcher.routes.Path = unavailableChildPath(t, "routes.jsonl")

	dispatcher.Handle(quotedMessage(t, "reply-route-error", "platform-1", "继续"))
	if len(queue.threadIDs) != 0 {
		t.Fatalf("unsafe dispatch without route storage: %#v", queue.threadIDs)
	}
	if len(*failures) != 1 || !strings.Contains((*failures)[0], "读取会话路由失败") {
		t.Fatalf("failure notices = %#v", *failures)
	}
}

func TestDispatcherReportsUnknownAgent(t *testing.T) {
	dispatcher, queue, failures := newTestDispatcher(t, config.AppConfig{ReplyEnabled: true})
	if err := dispatcher.routes.Record(Route{
		MessageID: "platform-unknown",
		BotID:     "bot-1",
		UserID:    "user-1",
		Agent:     "unknown",
		SessionID: "session-1",
	}); err != nil {
		t.Fatal(err)
	}

	dispatcher.Handle(quotedMessage(t, "reply-unknown", "platform-unknown", "继续"))
	if len(queue.threadIDs) != 0 {
		t.Fatalf("unknown agent dispatched through a fallback: %#v", queue.threadIDs)
	}
	if len(*failures) != 1 || !strings.Contains((*failures)[0], "暂不支持续聊 Agent") {
		t.Fatalf("failure notices = %#v", *failures)
	}
}

func TestDispatcherDeduplicatesByQuotedMessage(t *testing.T) {
	dispatcher, queue, failures := newTestDispatcher(t, config.AppConfig{ReplyEnabled: true})
	for index, messageID := range []string{"platform-1", "platform-2"} {
		if err := dispatcher.routes.Record(Route{
			MessageID: messageID,
			BotID:     "bot-1",
			UserID:    "user-1",
			Agent:     "codex",
			SessionID: fmt.Sprintf("thread-%d", index+1),
		}); err != nil {
			t.Fatalf("Record %s: %v", messageID, err)
		}
	}

	dispatcher.Handle(quotedMessage(t, "reply-1", "platform-1", "继续"))
	dispatcher.Handle(quotedMessage(t, "reply-2", "platform-2", "继续"))
	if len(queue.threadIDs) != 2 || queue.threadIDs[0] != "thread-1" || queue.threadIDs[1] != "thread-2" {
		t.Fatalf("thread IDs = %#v", queue.threadIDs)
	}
	if len(*failures) != 0 {
		t.Fatalf("failure notices = %#v", *failures)
	}
}

func TestDispatcherLogsSuccessfulDispatchWithoutReplyText(t *testing.T) {
	dispatcher, queue, failures := newTestDispatcher(t, config.AppConfig{ReplyEnabled: true})
	if err := dispatcher.routes.Record(Route{
		MessageID: "platform-1",
		BotID:     "bot-1",
		UserID:    "user-1",
		Agent:     "codex",
		SessionID: "thread-1",
	}); err != nil {
		t.Fatal(err)
	}

	const replyText = "机密回复内容-9271"
	dispatcher.Handle(quotedMessage(t, "reply-1", "platform-1", replyText))
	if len(queue.messages) != 1 || queue.messages[0] != replyText {
		t.Fatalf("messages = %#v", queue.messages)
	}
	if len(*failures) != 0 {
		t.Fatalf("failure notices = %#v", *failures)
	}

	diagnostics := readReplyDiagnostics(t)
	want := "dispatched quoted=platform-1 agent=codex session=thread-1"
	if !strings.Contains(diagnostics, want) {
		t.Fatalf("diagnostics = %q, want %q", diagnostics, want)
	}
	if strings.Contains(diagnostics, replyText) {
		t.Fatalf("diagnostics leaked reply text: %q", diagnostics)
	}
}

func TestDispatcherSendsDeliveryConfirmation(t *testing.T) {
	dispatcher, queue, notices := newTestDispatcher(t, config.AppConfig{ReplyEnabled: true, ReplyConfirmation: true})
	if err := dispatcher.routes.Record(Route{
		MessageID: "platform-1",
		BotID:     "bot-1",
		UserID:    "user-1",
		Agent:     "codex",
		SessionID: "thread-1",
		Title:     "重构登录页",
	}); err != nil {
		t.Fatal(err)
	}

	dispatcher.Handle(quotedMessage(t, "reply-1", "platform-1", "继续检查"))
	if len(queue.threadIDs) != 1 {
		t.Fatalf("reply not dispatched: %#v", queue.threadIDs)
	}
	if len(*notices) != 1 || (*notices)[0] != "✅ 已送达 **Codex**，会话：重构登录页" {
		t.Fatalf("delivery confirmation = %#v", *notices)
	}
}

func TestDispatcherDeliveryConfirmationFallsBackToSessionID(t *testing.T) {
	dispatcher, _, notices := newTestDispatcher(t, config.AppConfig{ReplyEnabled: true, ReplyConfirmation: true})
	if err := dispatcher.routes.Record(Route{
		MessageID: "platform-1",
		BotID:     "bot-1",
		UserID:    "user-1",
		Agent:     "codex",
		SessionID: "0123456789abcdef",
	}); err != nil {
		t.Fatal(err)
	}

	dispatcher.Handle(quotedMessage(t, "reply-1", "platform-1", "继续检查"))
	if len(*notices) != 1 || (*notices)[0] != "✅ 已送达 **Codex**，会话：…89abcdef" {
		t.Fatalf("delivery confirmation = %#v", *notices)
	}
}

func TestDispatcherSkipsDeliveryConfirmationWhenDisabled(t *testing.T) {
	dispatcher, queue, notices := newTestDispatcher(t, config.AppConfig{ReplyEnabled: true})
	if err := dispatcher.routes.Record(Route{
		MessageID: "platform-1",
		BotID:     "bot-1",
		UserID:    "user-1",
		Agent:     "codex",
		SessionID: "thread-1",
		Title:     "重构登录页",
	}); err != nil {
		t.Fatal(err)
	}

	dispatcher.Handle(quotedMessage(t, "reply-1", "platform-1", "继续检查"))
	if len(queue.threadIDs) != 1 {
		t.Fatalf("reply not dispatched: %#v", queue.threadIDs)
	}
	if len(*notices) != 0 {
		t.Fatalf("confirmation should be disabled: %#v", *notices)
	}
}

func unavailableChildPath(t *testing.T, child string) string {
	t.Helper()
	parent := filepath.Join(t.TempDir(), "not-a-directory")
	if err := os.WriteFile(parent, []byte("blocked"), 0600); err != nil {
		t.Fatalf("create unavailable parent: %v", err)
	}
	return filepath.Join(parent, child)
}

func readReplyDiagnostics(t *testing.T) string {
	t.Helper()
	logPath := filepath.Join(os.Getenv("AGENT_NOTIFY_TEMP_DIR"), "reply-debug.log")
	data, err := os.ReadFile(logPath)
	if err != nil {
		t.Fatalf("read reply diagnostics: %v", err)
	}
	return string(data)
}
