package clawbot

import (
	"context"
	"encoding/json"
	"errors"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"
)

func TestFetchQRCodeUsesPost(t *testing.T) {
	var method string
	var captured qrCodeRequest
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.URL.Path != "/ilink/bot/get_bot_qrcode" || r.URL.Query().Get("bot_type") != "3" {
			t.Fatalf("unexpected endpoint: %s?%s", r.URL.Path, r.URL.RawQuery)
		}
		method = r.Method
		_ = json.NewDecoder(r.Body).Decode(&captured)
		_, _ = w.Write([]byte(`{"qrcode":"qr-1","qrcode_img_content":"https://weixin.qq.com/qr-1","ret":0}`))
	}))
	defer server.Close()

	client := NewAuthClient(server.URL)
	response, err := client.FetchQRCode(context.Background(), []string{"token-old", "token-old", ""})
	if err != nil {
		t.Fatalf("FetchQRCode: %v", err)
	}
	if method != http.MethodPost {
		t.Fatalf("method = %s, want POST", method)
	}
	if len(captured.LocalTokenList) != 1 || captured.LocalTokenList[0] != "token-old" {
		t.Fatalf("local token list = %#v", captured.LocalTokenList)
	}
	if captured.BaseInfo.ChannelVersion != ChannelVersion || captured.BaseInfo.BotAgent == "" {
		t.Fatalf("unexpected base info: %#v", captured.BaseInfo)
	}
	if response.QRCode != "qr-1" {
		t.Fatalf("unexpected qr code: %#v", response)
	}
	if response.DisplayContent() != "https://weixin.qq.com/qr-1" {
		t.Fatalf("display content = %q", response.DisplayContent())
	}
}

func TestFetchQRCodeFallsBackToGet(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.Method == http.MethodPost {
			_, _ = w.Write([]byte(`{"ret":0}`))
			return
		}
		_, _ = w.Write([]byte(`{"qrcode":"qr-get","ret":0}`))
	}))
	defer server.Close()

	client := NewAuthClient(server.URL)
	response, err := client.FetchQRCode(context.Background(), nil)
	if err != nil {
		t.Fatalf("FetchQRCode: %v", err)
	}
	if response.QRCode != "qr-get" {
		t.Fatalf("unexpected qr code: %#v", response)
	}
	if response.DisplayContent() != "qr-get" {
		t.Fatalf("display content = %q", response.DisplayContent())
	}
}

func TestFetchQRCodePostFailureDoesNotFallBack(t *testing.T) {
	requests := 0
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		requests++
		http.Error(w, "unavailable", http.StatusInternalServerError)
	}))
	defer server.Close()

	client := NewAuthClient(server.URL)
	if _, err := client.FetchQRCode(context.Background(), nil); err == nil {
		t.Fatal("FetchQRCode succeeded, want POST error")
	}
	if requests != 1 {
		t.Fatalf("requests = %d, want only the POST attempt", requests)
	}
}

func TestPollQRStatusVerifyCodeFlow(t *testing.T) {
	var verifyCodes []string
	requests := 0
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		requests++
		verifyCodes = append(verifyCodes, r.URL.Query().Get("verify_code"))
		switch requests {
		case 1:
			_, _ = w.Write([]byte(`{"status":"wait","ret":0}`))
		case 2:
			_, _ = w.Write([]byte(`{"status":"need_verifycode","ret":0}`))
		default:
			_, _ = w.Write([]byte(`{"status":"confirmed","bot_token":"token-1","ilink_bot_id":"bot-1","baseurl":"https://example.test","ilink_user_id":"user-123456","ret":0}`))
		}
	}))
	defer server.Close()

	client := NewAuthClient(server.URL)
	var statuses []string
	var retries []bool
	credentials, err := client.PollQRStatus(context.Background(), "qr-1", PollOptions{
		OnStatus: func(status string) { statuses = append(statuses, status) },
		VerifyCode: func(retry bool) (string, error) {
			retries = append(retries, retry)
			return "1234", nil
		},
	})
	if err != nil {
		t.Fatalf("PollQRStatus: %v", err)
	}
	if credentials.BotToken != "token-1" || credentials.ILinkBotID != "bot-1" {
		t.Fatalf("unexpected credentials: %#v", credentials)
	}
	if credentials.BaseURL != "https://example.test" || credentials.ILinkUserID != "user-123456" {
		t.Fatalf("unexpected credentials: %#v", credentials)
	}
	if len(statuses) != 3 || statuses[0] != StatusWait || statuses[1] != StatusNeedVerifyCode || statuses[2] != StatusConfirmed {
		t.Fatalf("unexpected statuses: %#v", statuses)
	}
	if len(retries) != 1 || retries[0] {
		t.Fatalf("unexpected verify code retries: %#v", retries)
	}
	if len(verifyCodes) != 3 || verifyCodes[0] != "" || verifyCodes[1] != "" || verifyCodes[2] != "1234" {
		t.Fatalf("unexpected verify codes: %#v", verifyCodes)
	}
}

func TestPollQRStatusRedirectsToNewHost(t *testing.T) {
	target := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		_, _ = w.Write([]byte(`{"status":"confirmed","bot_token":"token-2","ilink_bot_id":"bot-2","ilink_user_id":"user-2","ret":0}`))
	}))
	defer target.Close()

	origin := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		_, _ = w.Write([]byte(`{"status":"scaned_but_redirect","redirect_host":"` + target.URL + `","ret":0}`))
	}))
	defer origin.Close()

	client := NewAuthClient(origin.URL)
	credentials, err := client.PollQRStatus(context.Background(), "qr-1", PollOptions{})
	if err != nil {
		t.Fatalf("PollQRStatus: %v", err)
	}
	if credentials.BotToken != "token-2" {
		t.Fatalf("unexpected credentials: %#v", credentials)
	}
	if credentials.BaseURL != target.URL {
		t.Fatalf("base url = %q, want redirect host %q", credentials.BaseURL, target.URL)
	}
}

func TestPollQRStatusBindedRedirectReusesLocalToken(t *testing.T) {
	dir := t.TempDir()
	t.Setenv("AGENT_NOTIFY_CONFIG_DIR", dir)
	existing := Credentials{
		BotToken:      "token-local",
		ILinkBotID:    "bot-local",
		BaseURL:       "https://example.test",
		ILinkUserID:   "user-123456",
		ContextToken:  "ctx-local",
		ContextUserID: "user-123456",
	}
	if err := SaveCredentials(existing); err != nil {
		t.Fatalf("SaveCredentials: %v", err)
	}

	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		_, _ = w.Write([]byte(`{"status":"binded_redirect","ret":0}`))
	}))
	defer server.Close()

	client := NewAuthClient(server.URL)
	credentials, err := client.PollQRStatus(context.Background(), "qr-1", PollOptions{})
	if err != nil {
		t.Fatalf("PollQRStatus: %v", err)
	}
	if credentials.BotToken != "token-local" || credentials.ContextToken != "ctx-local" {
		t.Fatalf("unexpected reused credentials: %#v", credentials)
	}
}

func TestPollQRStatusBindedRedirectWithoutLocalToken(t *testing.T) {
	t.Setenv("AGENT_NOTIFY_CONFIG_DIR", t.TempDir())
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		_, _ = w.Write([]byte(`{"status":"binded_redirect","ret":0}`))
	}))
	defer server.Close()

	client := NewAuthClient(server.URL)
	_, err := client.PollQRStatus(context.Background(), "qr-1", PollOptions{})
	if !errors.Is(err, ErrAlreadyBound) {
		t.Fatalf("error = %v, want ErrAlreadyBound", err)
	}
}

func TestPollQRStatusExpired(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		_, _ = w.Write([]byte(`{"status":"expired","ret":0}`))
	}))
	defer server.Close()

	client := NewAuthClient(server.URL)
	_, err := client.PollQRStatus(context.Background(), "qr-1", PollOptions{})
	if !errors.Is(err, ErrQRCodeExpired) {
		t.Fatalf("error = %v, want ErrQRCodeExpired", err)
	}
}

func TestPollQRStatusRequiresVerifyCodeHandler(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		_, _ = w.Write([]byte(`{"status":"need_verifycode","ret":0}`))
	}))
	defer server.Close()

	client := NewAuthClient(server.URL)
	_, err := client.PollQRStatus(context.Background(), "qr-1", PollOptions{})
	if err == nil || !strings.Contains(err.Error(), "配对码") {
		t.Fatalf("error = %v, want pairing-code failure", err)
	}
}

func TestCredentialsLifecycle(t *testing.T) {
	dir := t.TempDir()
	t.Setenv("AGENT_NOTIFY_CONFIG_DIR", dir)

	want := Credentials{
		BotToken:      "token-1",
		ILinkBotID:    "bot-1",
		BaseURL:       "https://example.test",
		ILinkUserID:   "user-123456",
		ContextToken:  "ctx-1",
		ContextUserID: "user-123456",
		GetUpdatesBuf: "cursor-1",
	}
	if err := SaveCredentials(want); err != nil {
		t.Fatalf("SaveCredentials: %v", err)
	}
	if !HasCredentials() {
		t.Fatal("HasCredentials = false after saving")
	}
	got, err := LoadCredentials()
	if err != nil {
		t.Fatalf("LoadCredentials: %v", err)
	}
	if got != want {
		t.Fatalf("LoadCredentials = %#v, want %#v", got, want)
	}
	status := GetStatus()
	if !status.LoggedIn || !status.SessionReady || status.Stale {
		t.Fatalf("unexpected status: %#v", status)
	}
	if status.UserHint != "user...3456" {
		t.Fatalf("unexpected user hint: %q", status.UserHint)
	}

	if err := markStaleIfToken(want.BotToken); err != nil {
		t.Fatalf("markStaleIfToken: %v", err)
	}
	status = GetStatus()
	if !status.LoggedIn || !status.Stale || status.SessionReady {
		t.Fatalf("unexpected stale status: %#v", status)
	}
	stale, err := LoadCredentials()
	if err != nil {
		t.Fatalf("LoadCredentials: %v", err)
	}
	if stale.ContextToken != "" || stale.ContextUserID != "" {
		t.Fatalf("stale credentials kept context: %#v", stale)
	}
	if _, err := NewClient(stale); !errors.Is(err, ErrStaleToken) {
		t.Fatalf("NewClient error = %v, want ErrStaleToken", err)
	}

	if err := DeleteCredentials(); err != nil {
		t.Fatalf("DeleteCredentials: %v", err)
	}
	if HasCredentials() {
		t.Fatal("HasCredentials = true after delete")
	}
}

func TestUpdateCredentialsPreservesLogin(t *testing.T) {
	t.Setenv("AGENT_NOTIFY_CONFIG_DIR", t.TempDir())
	if err := SaveCredentials(boundCredentials()); err != nil {
		t.Fatalf("SaveCredentials: %v", err)
	}
	if err := updateCredentials(func(credentials *Credentials) error {
		credentials.GetUpdatesBuf = "cursor-9"
		credentials.ContextToken = "ctx-9"
		return nil
	}); err != nil {
		t.Fatalf("updateCredentials: %v", err)
	}
	got, err := LoadCredentials()
	if err != nil {
		t.Fatalf("LoadCredentials: %v", err)
	}
	if got.BotToken != "token-1" || got.ILinkBotID != "bot-1" {
		t.Fatalf("login fields changed: %#v", got)
	}
	if got.GetUpdatesBuf != "cursor-9" || got.ContextToken != "ctx-9" {
		t.Fatalf("session fields not saved: %#v", got)
	}
}

func TestRenderQR(t *testing.T) {
	rendered, err := RenderQR("weixin://qr/test")
	if err != nil {
		t.Fatalf("RenderQR: %v", err)
	}
	if len([]rune(rendered)) < 100 {
		t.Fatalf("rendered QR is unexpectedly small: %d runes", len([]rune(rendered)))
	}
}
