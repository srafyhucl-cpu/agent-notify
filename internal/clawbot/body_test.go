package clawbot

import (
	"bytes"
	"io"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"
)

func TestReadResponseBodyWithinLimit(t *testing.T) {
	payload := bytes.Repeat([]byte("a"), maxResponseBytes)
	data, err := readResponseBody(bytes.NewReader(payload))
	if err != nil {
		t.Fatalf("readResponseBody at limit: %v", err)
	}
	if len(data) != maxResponseBytes {
		t.Fatalf("readResponseBody length = %d, want %d", len(data), maxResponseBytes)
	}
}

func TestReadResponseBodyExceedsLimit(t *testing.T) {
	payload := bytes.Repeat([]byte("a"), maxResponseBytes+1)
	_, err := readResponseBody(bytes.NewReader(payload))
	if err == nil || !strings.Contains(err.Error(), "exceeds") {
		t.Fatalf("readResponseBody over limit error = %v, want size error", err)
	}
}

func TestReadResponseBodyFromServer(t *testing.T) {
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		_, _ = io.Copy(w, bytes.NewReader(bytes.Repeat([]byte("a"), maxResponseBytes+1)))
	}))
	defer server.Close()

	resp, err := http.Get(server.URL)
	if err != nil {
		t.Fatalf("http.Get: %v", err)
	}
	defer resp.Body.Close()

	if _, err := readResponseBody(resp.Body); err == nil || !strings.Contains(err.Error(), "exceeds") {
		t.Fatalf("readResponseBody error = %v, want size error", err)
	}
}
