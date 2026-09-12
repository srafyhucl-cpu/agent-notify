package clawbot

import (
	"context"
	"net/http"
	"net/http/httptest"
	"testing"
)

func TestAuthClientFetchAndPoll(t *testing.T) {
	requests := 0
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		switch r.URL.Path {
		case "/ilink/bot/get_bot_qrcode":
			_, _ = w.Write([]byte(`{"qrcode":"qr-1","qrcode_img_content":"weixin://qr-1","ret":0}`))
		case "/ilink/bot/get_qrcode_status":
			requests++
			if requests == 1 {
				_, _ = w.Write([]byte(`{"status":"wait","ret":0}`))
				return
			}
			_, _ = w.Write([]byte(`{"status":"confirmed","bot_token":"token-1","ilink_bot_id":"bot-1","baseurl":"http://example.test","ilink_user_id":"user-123456","ret":0}`))
		default:
			t.Fatalf("unexpected path: %s", r.URL.Path)
		}
	}))
	defer server.Close()

	client := NewAuthClient(server.URL)
	qr, err := client.FetchQRCode(context.Background())
	if err != nil {
		t.Fatalf("FetchQRCode returned error: %v", err)
	}
	if qr.QRCode != "qr-1" {
		t.Fatalf("unexpected qr code: %#v", qr)
	}

	var statuses []string
	creds, err := client.PollQRStatus(context.Background(), qr.QRCode, func(status string) {
		statuses = append(statuses, status)
	})
	if err != nil {
		t.Fatalf("PollQRStatus returned error: %v", err)
	}
	if len(statuses) != 2 || statuses[0] != StatusWait || statuses[1] != StatusConfirmed {
		t.Fatalf("unexpected statuses: %#v", statuses)
	}
	if creds.BotToken != "token-1" || creds.ILinkUserID != "user-123456" {
		t.Fatalf("unexpected credentials: %#v", creds)
	}
}

func TestCredentialsLifecycle(t *testing.T) {
	dir := t.TempDir()
	t.Setenv("AGENT_NOTIFY_CONFIG_DIR", dir)

	want := Credentials{
		BotToken:    "token-1",
		ILinkBotID:  "bot-1",
		BaseURL:     "https://example.test",
		ILinkUserID: "user-123456",
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
	if status := GetStatus(); !status.LoggedIn || status.UserHint != "user...3456" {
		t.Fatalf("unexpected status: %#v", status)
	}
	if err := DeleteCredentials(); err != nil {
		t.Fatalf("DeleteCredentials: %v", err)
	}
	if HasCredentials() {
		t.Fatal("HasCredentials = true after delete")
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
