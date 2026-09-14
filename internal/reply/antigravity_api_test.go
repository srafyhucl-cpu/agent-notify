package reply

import (
	"context"
	"errors"
	"strings"
	"testing"
)

type fakeAgentAPI struct {
	endpoint  AntigravityEndpoint
	sessionID string
	text      string
	sendErr   error
}

func (a *fakeAgentAPI) ConversationExists(
	_ context.Context,
	endpoint AntigravityEndpoint,
	sessionID string,
) (bool, error) {
	a.endpoint = endpoint
	a.sessionID = sessionID
	return true, nil
}

func (a *fakeAgentAPI) SendMessage(
	_ context.Context,
	endpoint AntigravityEndpoint,
	sessionID, text string,
) error {
	a.endpoint = endpoint
	a.sessionID = sessionID
	a.text = text
	return a.sendErr
}

func TestAntigravitySenderUsesResolvedEndpointAndExactSession(t *testing.T) {
	endpoint := AntigravityEndpoint{Address: "127.0.0.1:62957", Token: "csrf-token"}
	api := &fakeAgentAPI{}
	sender := AntigravityAgentAPISender{
		Resolver: func(context.Context, string) (AntigravityEndpoint, error) {
			return endpoint, nil
		},
		API: api,
	}

	if err := sender.Send(context.Background(), "conversation-1", "继续检查"); err != nil {
		t.Fatalf("Send: %v", err)
	}
	if api.endpoint != endpoint || api.sessionID != "conversation-1" || api.text != "继续检查" {
		t.Fatalf("send call = endpoint=%#v session=%q text=%q", api.endpoint, api.sessionID, api.text)
	}
}

func TestAntigravitySenderClassifiesExpiredToken(t *testing.T) {
	sender := AntigravityAgentAPISender{
		Resolver: func(context.Context, string) (AntigravityEndpoint, error) {
			return AntigravityEndpoint{Address: "127.0.0.1:62957", Token: "old"}, nil
		},
		API: &fakeAgentAPI{sendErr: errors.New("CSRF token rejected")},
	}

	err := sender.Send(context.Background(), "conversation-1", "继续")
	if err == nil || !strings.Contains(err.Error(), "令牌已失效") {
		t.Fatalf("Send error = %v, want expired-token guidance", err)
	}
}

func TestAntigravitySenderDoesNotRetryAfterResolutionFailure(t *testing.T) {
	calls := 0
	sender := AntigravityAgentAPISender{
		Resolver: func(context.Context, string) (AntigravityEndpoint, error) {
			calls++
			return AntigravityEndpoint{}, errors.New("language server unavailable")
		},
		API: &fakeAgentAPI{},
	}
	if err := sender.Send(context.Background(), "conversation-1", "继续"); err == nil {
		t.Fatal("Send succeeded, want resolver failure")
	}
	if calls != 1 {
		t.Fatalf("resolver calls = %d, want 1", calls)
	}
}

func TestSelectAntigravityEndpointChoosesInstanceWithSession(t *testing.T) {
	first := AntigravityEndpoint{Address: "127.0.0.1:1", Token: "one"}
	second := AntigravityEndpoint{Address: "127.0.0.1:2", Token: "two"}
	api := &selectAgentAPI{matches: map[string]bool{second.Address: true}}
	got, err := selectAntigravityEndpoint(
		context.Background(),
		api,
		[]AntigravityEndpoint{first, second},
		"conversation-1",
	)
	if err != nil {
		t.Fatalf("selectAntigravityEndpoint: %v", err)
	}
	if got != second {
		t.Fatalf("endpoint = %#v, want %#v", got, second)
	}
}

type selectAgentAPI struct {
	matches map[string]bool
}

func (a *selectAgentAPI) ConversationExists(
	_ context.Context,
	endpoint AntigravityEndpoint,
	_ string,
) (bool, error) {
	return a.matches[endpoint.Address], nil
}

func (a *selectAgentAPI) SendMessage(
	context.Context,
	AntigravityEndpoint,
	string,
	string,
) error {
	return nil
}
