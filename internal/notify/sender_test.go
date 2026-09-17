package notify

import (
	"context"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/srafyhucl-cpu/agent-notify/internal/clawbot"
	"github.com/srafyhucl-cpu/agent-notify/internal/config"
	"github.com/srafyhucl-cpu/agent-notify/internal/reply"
)

type recordingReplyQueue struct {
	threadIDs []string
	messages  []string
}

func (q *recordingReplyQueue) Queue(_ context.Context, threadID, text string) error {
	q.threadIDs = append(q.threadIDs, threadID)
	q.messages = append(q.messages, text)
	return nil
}

func TestSendNotificationDryRun(t *testing.T) {
	result := SendNotification(NotifyOptions{
		Title:   "测试",
		Summary: "hello **world**",
		DryRun:  true,
	})
	if result.Status != StatusDryRun {
		t.Fatalf("Status = %q, want %q", result.Status, StatusDryRun)
	}
	var payload map[string]string
	if err := json.Unmarshal([]byte(result.DryRunPayload), &payload); err != nil {
		t.Fatalf("decode dry-run payload: %v", err)
	}
	if payload["message"] != "🟢【通知】测试\n\nhello **world**" {
		t.Fatalf("unexpected dry-run message: %q", payload["message"])
	}
}

func TestSendNotificationWithoutLogin(t *testing.T) {
	t.Setenv("AGENT_NOTIFY_CONFIG_DIR", t.TempDir())
	t.Setenv("AGENT_NOTIFY_TEMP_DIR", t.TempDir())
	result := SendNotification(NotifyOptions{Title: "测试", Summary: "hello"})
	if result.Status != StatusNotLoggedIn {
		t.Fatalf("Status = %q, want %q", result.Status, StatusNotLoggedIn)
	}
}

func TestSendNotificationWithoutSession(t *testing.T) {
	dir := t.TempDir()
	t.Setenv("AGENT_NOTIFY_CONFIG_DIR", dir)
	t.Setenv("AGENT_NOTIFY_TEMP_DIR", filepath.Join(dir, "temp"))
	if err := clawbot.SaveCredentials(clawbot.Credentials{
		BotToken:    "token",
		ILinkBotID:  "bot",
		ILinkUserID: "user",
	}); err != nil {
		t.Fatalf("SaveCredentials: %v", err)
	}

	result := SendNotification(NotifyOptions{Title: "测试", Summary: "hello"})
	if result.Status != StatusSessionMissing {
		t.Fatalf("Status = %q, want %q", result.Status, StatusSessionMissing)
	}
	if result.Error == "" {
		t.Fatal("expected actionable session error")
	}
}

func TestSendNotificationSuccess(t *testing.T) {
	var message string
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		var request struct {
			Msg struct {
				ItemList []struct {
					TextItem struct {
						Text string `json:"text"`
					} `json:"text_item"`
				} `json:"item_list"`
			} `json:"msg"`
		}
		_ = json.NewDecoder(r.Body).Decode(&request)
		if len(request.Msg.ItemList) > 0 {
			message = request.Msg.ItemList[0].TextItem.Text
		}
		_ = json.NewEncoder(w).Encode(map[string]any{"ret": 0})
	}))
	defer server.Close()

	dir := t.TempDir()
	t.Setenv("AGENT_NOTIFY_CONFIG_DIR", dir)
	t.Setenv("AGENT_NOTIFY_TEMP_DIR", filepath.Join(dir, "temp"))
	if err := clawbot.SaveCredentials(clawbot.Credentials{
		BotToken:      "token",
		ILinkBotID:    "bot",
		BaseURL:       server.URL,
		ILinkUserID:   "user",
		ContextToken:  "context",
		ContextUserID: "user",
	}); err != nil {
		t.Fatalf("SaveCredentials: %v", err)
	}

	result := SendNotification(NotifyOptions{
		Agent:     "opencode",
		SessionID: "session-1",
		Title:     "任务完成",
		Summary:   "all good",
	})
	if result.Status != StatusSuccess {
		t.Fatalf("Status = %q, error = %q", result.Status, result.Error)
	}
	if !strings.HasPrefix(message, "🟢【OpenCode】任务完成\n\nall good\n\n---\n> 微信直接引用此消息可继续对话\n\nOpenCode · ") {
		t.Fatalf("sent message = %q", message)
	}

	history, err := GetHistory(10, "")
	if err != nil {
		t.Fatalf("GetHistory: %v", err)
	}
	if len(history) != 1 || history[0].Status != StatusSuccess {
		t.Fatalf("history = %#v", history)
	}
}

func TestSendNotificationRecordsReplyRoute(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		_ = json.NewEncoder(w).Encode(map[string]any{
			"ret": 0, "message_id": "platform-1", "client_id": "client-1",
		})
	}))
	defer server.Close()

	dir := t.TempDir()
	t.Setenv("AGENT_NOTIFY_CONFIG_DIR", dir)
	t.Setenv("AGENT_NOTIFY_TEMP_DIR", filepath.Join(dir, "temp"))
	if err := clawbot.SaveCredentials(clawbot.Credentials{
		BotToken:      "token",
		ILinkBotID:    "bot-1",
		BaseURL:       server.URL,
		ILinkUserID:   "user-1",
		ContextToken:  "context",
		ContextUserID: "user-1",
	}); err != nil {
		t.Fatalf("SaveCredentials: %v", err)
	}

	result := SendNotification(NotifyOptions{
		Agent:     "codex",
		SessionID: "thread-1",
		Title:     "任务完成",
		Summary:   "hello",
	})
	if result.Status != StatusSuccess || result.MessageID != "platform-1" || !strings.HasPrefix(result.ClientID, "agent-notify-") {
		t.Fatalf("result = %#v", result)
	}
	route, err := reply.NewRouteStore("").Find("bot-1", "user-1", "platform-1", "")
	if err != nil {
		t.Fatalf("Find route: %v", err)
	}
	if route.Agent != "codex" || route.SessionID != "thread-1" {
		t.Fatalf("route = %#v", route)
	}

	history, err := GetHistory(1, "")
	if err != nil || len(history) != 1 || history[0].MessageID != "platform-1" || !strings.HasPrefix(history[0].ClientID, "agent-notify-") {
		t.Fatalf("history = %#v, err=%v", history, err)
	}
}

func TestSendNotificationExpiredSessionIsCleared(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		_ = json.NewEncoder(w).Encode(map[string]any{
			"ret": -2, "errmsg": "prepare failed",
		})
	}))
	defer server.Close()

	dir := t.TempDir()
	t.Setenv("AGENT_NOTIFY_CONFIG_DIR", dir)
	t.Setenv("AGENT_NOTIFY_TEMP_DIR", filepath.Join(dir, "temp"))
	if err := clawbot.SaveCredentials(clawbot.Credentials{
		BotToken:      "token",
		ILinkBotID:    "bot",
		BaseURL:       server.URL,
		ILinkUserID:   "user",
		ContextToken:  "context",
		ContextUserID: "user",
	}); err != nil {
		t.Fatalf("SaveCredentials: %v", err)
	}

	result := SendNotification(NotifyOptions{Title: "测试", Summary: "hello"})
	if result.Status != StatusSessionMissing {
		t.Fatalf("Status = %q, error = %q", result.Status, result.Error)
	}
	if !strings.Contains(result.Error, "主动推送会话已失效") {
		t.Fatalf("unexpected error: %q", result.Error)
	}

	credentials, err := clawbot.LoadCredentials()
	if err != nil {
		t.Fatalf("LoadCredentials: %v", err)
	}
	if credentials.ContextToken != "" || credentials.ContextUserID != "" {
		t.Fatalf("expired context was not cleared: %#v", credentials)
	}
	if status := clawbot.GetStatus(); status.SessionReady {
		t.Fatalf("session status still reports ready: %#v", status)
	}
}

func TestSendNotificationRecordsClientIDFallbackRoute(t *testing.T) {
	var requestClientID string
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		var request struct {
			Msg struct {
				ClientID string `json:"client_id"`
			} `json:"msg"`
		}
		if err := json.NewDecoder(r.Body).Decode(&request); err != nil {
			t.Fatalf("decode request: %v", err)
		}
		requestClientID = request.Msg.ClientID
		_ = json.NewEncoder(w).Encode(map[string]any{"ret": 0})
	}))
	defer server.Close()

	dir := t.TempDir()
	t.Setenv("AGENT_NOTIFY_CONFIG_DIR", dir)
	t.Setenv("AGENT_NOTIFY_TEMP_DIR", filepath.Join(dir, "temp"))
	if err := clawbot.SaveCredentials(clawbot.Credentials{
		BotToken:      "token",
		ILinkBotID:    "bot-1",
		BaseURL:       server.URL,
		ILinkUserID:   "user-1",
		ContextToken:  "context",
		ContextUserID: "user-1",
	}); err != nil {
		t.Fatalf("SaveCredentials: %v", err)
	}

	result := SendNotification(NotifyOptions{
		Agent:     "codex",
		SessionID: "thread-1",
		Title:     "任务完成",
		Summary:   "hello",
	})
	if result.Status != StatusSuccess || result.MessageID != "" || requestClientID == "" || result.ClientID != requestClientID {
		t.Fatalf("result = %#v, requestClientID = %q", result, requestClientID)
	}

	route, err := reply.NewRouteStore("").Find("bot-1", "user-1", "", requestClientID)
	if err != nil {
		t.Fatalf("Find client-ID route: %v", err)
	}
	if route.Agent != "codex" || route.SessionID != "thread-1" {
		t.Fatalf("route = %#v", route)
	}
}

func TestSendNotificationRouteDispatchesExactQuotedReply(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		var request struct {
			Msg struct {
				ItemList []struct {
					TextItem struct {
						Text string `json:"text"`
					} `json:"text_item"`
				} `json:"item_list"`
			} `json:"msg"`
		}
		_ = json.NewDecoder(r.Body).Decode(&request)
		response := map[string]any{"ret": 0}
		if len(request.Msg.ItemList) == 0 || !strings.Contains(request.Msg.ItemList[0].TextItem.Text, "client-id-route") {
			response["message_id"] = "platform-1"
			response["client_id"] = "server-client-1"
		}
		_ = json.NewEncoder(w).Encode(response)
	}))
	defer server.Close()

	dir := t.TempDir()
	t.Setenv("AGENT_NOTIFY_CONFIG_DIR", dir)
	t.Setenv("AGENT_NOTIFY_TEMP_DIR", filepath.Join(dir, "temp"))
	if err := clawbot.SaveCredentials(clawbot.Credentials{
		BotToken:      "token",
		ILinkBotID:    "bot-1",
		BaseURL:       server.URL,
		ILinkUserID:   "user-1",
		ContextToken:  "context",
		ContextUserID: "user-1",
	}); err != nil {
		t.Fatalf("SaveCredentials: %v", err)
	}

	sent := SendNotification(NotifyOptions{
		Agent:     "codex",
		SessionID: "thread-1",
		Title:     "任务完成",
		Summary:   "hello",
	})
	if sent.Status != StatusSuccess || sent.MessageID != "platform-1" {
		t.Fatalf("send result = %#v", sent)
	}

	clientIDRoute := SendNotification(NotifyOptions{
		Agent:     "codex",
		SessionID: "thread-2",
		Title:     "client-id-route",
		Summary:   "hello",
	})
	if clientIDRoute.Status != StatusSuccess || clientIDRoute.MessageID != "" || clientIDRoute.ClientID == "" {
		t.Fatalf("client-ID route send result = %#v", clientIDRoute)
	}

	queue := &recordingReplyQueue{}
	var failures []string
	dispatcher := reply.NewDispatcher(reply.DispatcherOptions{
		Routes: reply.NewRouteStore(""),
		State:  reply.NewStateStore(""),
		Load: func() (config.AppConfig, error) {
			return config.AppConfig{ReplyEnabled: true}, nil
		},
		Senders: map[string]reply.ReplySender{
			"codex": reply.CodexReplySender{Queue: queue},
		},
		SendText: func(_ context.Context, text string) error {
			failures = append(failures, text)
			return nil
		},
	})

	var inbound clawbot.InboundMessage
	raw := `{
		"msg_id": "reply-1",
		"seq": 9,
		"from_user_id": "user-1",
		"message_type": 1,
		"item_list": [
			{"type": 1, "text_item": {"text": "  继续检查  "}},
			{"type": 3, "ref_msg": {"message_item": {"msg_id": "platform-1"}}}
		]
	}`
	if err := json.NewDecoder(strings.NewReader(raw)).Decode(&inbound); err != nil {
		t.Fatalf("decode inbound reply: %v", err)
	}

	dispatcher.Handle(inbound)
	if len(queue.threadIDs) != 1 || queue.threadIDs[0] != "thread-1" {
		t.Fatalf("thread IDs = %#v", queue.threadIDs)
	}
	if len(queue.messages) != 1 || queue.messages[0] != "继续检查" {
		t.Fatalf("messages = %#v", queue.messages)
	}

	var clientIDReply clawbot.InboundMessage
	raw = `{
		"msg_id": "reply-2",
		"seq": 10,
		"from_user_id": "user-1",
		"message_type": 1,
		"item_list": [
			{"type": 1, "text_item": {"text": "线程二"}},
			{"type": 3, "ref_msg": {"message_item": {"msg_id": "` + clientIDRoute.ClientID + `"}}}
		]
	}`
	if err := json.NewDecoder(strings.NewReader(raw)).Decode(&clientIDReply); err != nil {
		t.Fatalf("decode client-ID reply: %v", err)
	}
	dispatcher.Handle(clientIDReply)
	if len(queue.threadIDs) != 2 || queue.threadIDs[1] != "thread-2" {
		t.Fatalf("client-ID thread IDs = %#v", queue.threadIDs)
	}
	if len(queue.messages) != 2 || queue.messages[1] != "线程二" {
		t.Fatalf("client-ID messages = %#v", queue.messages)
	}

	var unmatched clawbot.InboundMessage
	raw = `{
		"msg_id": "reply-3",
		"seq": 11,
		"from_user_id": "user-1",
		"message_type": 1,
		"item_list": [
			{"type": 1, "text_item": {"text": "错误目标"}},
			{"type": 3, "ref_msg": {"message_item": {"msg_id": "platform-2"}}}
		]
	}`
	if err := json.NewDecoder(strings.NewReader(raw)).Decode(&unmatched); err != nil {
		t.Fatalf("decode unmatched reply: %v", err)
	}
	dispatcher.Handle(unmatched)
	if len(queue.threadIDs) != 2 || len(queue.messages) != 2 {
		t.Fatalf("unknown reference fell back to another route: thread IDs=%#v messages=%#v", queue.threadIDs, queue.messages)
	}
	if len(failures) != 1 || !strings.Contains(failures[0], "没有可用的会话记录") {
		t.Fatalf("failure notices = %#v", failures)
	}
}

// blockPushLog 把推送历史路径指向一个目录，使后续 appendHistory 必然失败，
// 用于验证写入失败会通过 Warning 暴露而不是被静默忽略。
func blockPushLog(t *testing.T) {
	t.Helper()
	dir := t.TempDir()
	blocked := filepath.Join(dir, "push.log")
	if err := os.MkdirAll(blocked, 0700); err != nil {
		t.Fatalf("setup blocked push log: %v", err)
	}
	t.Setenv("AGENT_NOTIFY_LOG_FILE", blocked)
}

func TestRecordFailureReportsHistoryWriteError(t *testing.T) {
	blockPushLog(t)

	result := recordFailure(NotifyOptions{Title: "测试", Summary: "hello"}, "测试", "hello", StatusFailed, "发送失败")
	if result.Status != StatusFailed || result.Error != "发送失败" {
		t.Fatalf("result = %#v", result)
	}
	if result.Warning == "" {
		t.Fatal("history write failure must surface as non-empty Warning")
	}
}

func TestRecordSkippedReportsHistoryWriteError(t *testing.T) {
	blockPushLog(t)

	result := RecordSkipped(NotifyOptions{Title: "测试", Summary: "hello"}, "策略跳过")
	if result.Status != StatusSkipped || result.Error != "策略跳过" {
		t.Fatalf("result = %#v", result)
	}
	if result.Warning == "" {
		t.Fatal("history write failure must surface as non-empty Warning")
	}
}

func TestNotificationRoutingEligibility(t *testing.T) {
	tests := []struct {
		name   string
		opts   NotifyOptions
		result clawbot.SendResult
		want   bool
	}{
		{
			name:   "codex message id",
			opts:   NotifyOptions{Agent: "codex", SessionID: "thread-1"},
			result: clawbot.SendResult{MessageID: "platform-1"},
			want:   true,
		},
		{
			name:   "opencode client id",
			opts:   NotifyOptions{Agent: "opencode", SessionID: "session-1"},
			result: clawbot.SendResult{ClientID: "client-1"},
			want:   true,
		},
		{
			name:   "generic agent",
			opts:   NotifyOptions{Agent: "test", SessionID: "session-1"},
			result: clawbot.SendResult{MessageID: "platform-1"},
			want:   false,
		},
		{
			name:   "missing session",
			opts:   NotifyOptions{Agent: "codex"},
			result: clawbot.SendResult{MessageID: "platform-1"},
			want:   false,
		},
		{
			name: "missing identifiers",
			opts: NotifyOptions{Agent: "codex", SessionID: "thread-1"},
		},
	}

	for _, testCase := range tests {
		t.Run(testCase.name, func(t *testing.T) {
			if got := isRouteable(testCase.opts, testCase.result); got != testCase.want {
				t.Fatalf("isRouteable() = %v, want %v", got, testCase.want)
			}
		})
	}
}
