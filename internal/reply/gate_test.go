package reply

import (
	"encoding/json"
	"strings"
	"testing"

	"github.com/srafyhucl-cpu/agent-notify/internal/clawbot"
)

func gateEntry(t *testing.T, operation clawbot.DebugOperation, payload any) clawbot.DebugEntry {
	t.Helper()
	data, err := json.Marshal(payload)
	if err != nil {
		t.Fatalf("marshal payload: %v", err)
	}
	return clawbot.DebugEntry{Operation: operation, Data: json.RawMessage(data)}
}

func gateSendResult(t *testing.T, messageID, clientID string) clawbot.DebugEntry {
	t.Helper()
	return gateEntry(t, clawbot.DebugOperationSendResult, clawbot.DebugSendResult{
		MessageID: messageID,
		ClientID:  clientID,
	})
}

func gateInbound(t *testing.T, messageID string, referencedIDs ...string) clawbot.DebugEntry {
	t.Helper()
	return gateInboundReference(t, messageID, len(referencedIDs) > 0, referencedIDs...)
}

func gateInboundReference(t *testing.T, messageID string, hasReference bool, referencedIDs ...string) clawbot.DebugEntry {
	t.Helper()
	return gateEntry(t, clawbot.DebugOperationGetUpdatesData, []clawbot.DebugInboundReference{{
		MessageID:            messageID,
		HasReference:         hasReference,
		ReferencedMessageIDs: referencedIDs,
	}})
}

func gateScopedSendResult(t *testing.T, accountScope, messageID, clientID string) clawbot.DebugEntry {
	t.Helper()
	return gateEntry(t, clawbot.DebugOperationSendResult, clawbot.DebugSendResult{
		AccountScope: accountScope,
		MessageID:    messageID,
		ClientID:     clientID,
	})
}

func gateScopedInbound(t *testing.T, accountScope, messageID string, private, boundSender bool, referencedIDs ...string) clawbot.DebugEntry {
	t.Helper()
	return gateEntry(t, clawbot.DebugOperationGetUpdatesData, []clawbot.DebugInboundReference{{
		MessageID:            messageID,
		HasReference:         len(referencedIDs) > 0,
		ReferencedMessageIDs: referencedIDs,
		AccountScope:         accountScope,
		Private:              private,
		BoundSender:          boundSender,
	}})
}

func resolvingGateRoute(id string) (Route, error) {
	return Route{Agent: "codex", SessionID: "thread-" + id}, nil
}

func TestReplyGateRequiresSendEvidence(t *testing.T) {
	report := EvaluateReplyGate(nil, nil)
	if report.Status != ReplyGateAwaitingSend {
		t.Fatalf("status = %q, want %q", report.Status, ReplyGateAwaitingSend)
	}
	if len(report.Sends) != 0 || len(report.Quotes) != 0 {
		t.Fatalf("report = %#v", report)
	}

	report = EvaluateReplyGate([]clawbot.DebugEntry{gateInbound(t, "reply-1", "platform-1")}, nil)
	if report.Status != ReplyGateAwaitingSend {
		t.Fatalf("status without sends = %q, want %q", report.Status, ReplyGateAwaitingSend)
	}
	if len(report.Quotes) != 1 || report.FailedQuotes != 1 {
		t.Fatalf("quotes = %#v failed=%d", report.Quotes, report.FailedQuotes)
	}
}

func TestReplyGateWaitsForQuotedReply(t *testing.T) {
	report := EvaluateReplyGate([]clawbot.DebugEntry{gateSendResult(t, "platform-1", "client-1")}, nil)
	if report.Status != ReplyGateAwaitingQuote {
		t.Fatalf("status = %q, want %q", report.Status, ReplyGateAwaitingQuote)
	}
	if len(report.Sends) != 1 || report.Sends[0].MessageID != "platform-1" {
		t.Fatalf("sends = %#v", report.Sends)
	}
}

func TestReplyGatePassesOnPlatformIDMatch(t *testing.T) {
	entry := gateSendResult(t, "platform-1", "client-1")
	quote := gateInbound(t, "reply-1", "platform-1")
	report := EvaluateReplyGate([]clawbot.DebugEntry{entry, quote}, resolvingGateRoute)

	if report.Status != ReplyGatePassed {
		t.Fatalf("status = %q, want %q: %#v", report.Status, ReplyGatePassed, report)
	}
	if report.MatchedQuotes != 1 || report.FailedQuotes != 0 || report.RouteFailures != 0 {
		t.Fatalf("matched=%d failed=%d routeFailures=%d", report.MatchedQuotes, report.FailedQuotes, report.RouteFailures)
	}
	if got := report.Quotes[0]; got.Match != ReplyGateMatchPlatformID || got.RouteAgent != "codex" || got.RouteSession != "thread-platform-1" {
		t.Fatalf("quote = %#v", got)
	}
}

func TestReplyGatePassesOnClientIDMatch(t *testing.T) {
	report := EvaluateReplyGate([]clawbot.DebugEntry{
		gateSendResult(t, "", "client-1"),
		gateInbound(t, "reply-1", "client-1"),
	}, resolvingGateRoute)

	if report.Status != ReplyGatePassed {
		t.Fatalf("status = %q, want %q: %#v", report.Status, ReplyGatePassed, report)
	}
	if got := report.Quotes[0]; got.Match != ReplyGateMatchClientID || got.MatchedID != "client-1" {
		t.Fatalf("quote = %#v", got)
	}
}

func TestReplyGateFailsOnUnmatchedQuote(t *testing.T) {
	report := EvaluateReplyGate([]clawbot.DebugEntry{
		gateSendResult(t, "platform-1", "client-1"),
		gateInbound(t, "reply-1", "platform-2"),
	}, resolvingGateRoute)

	if report.Status != ReplyGateFailed {
		t.Fatalf("status = %q, want %q", report.Status, ReplyGateFailed)
	}
	if report.MatchedQuotes != 0 || report.FailedQuotes != 1 {
		t.Fatalf("matched=%d failed=%d", report.MatchedQuotes, report.FailedQuotes)
	}
	if got := report.Quotes[0]; got.Match != ReplyGateMatchNone || got.MatchedID != "" {
		t.Fatalf("quote = %#v", got)
	}
}

func TestReplyGateFailsWhenAnyQuoteIsUnmatched(t *testing.T) {
	report := EvaluateReplyGate([]clawbot.DebugEntry{
		gateSendResult(t, "platform-1", "client-1"),
		gateInbound(t, "reply-1", "platform-1"),
		gateInbound(t, "reply-2", "unknown"),
	}, resolvingGateRoute)

	if report.Status != ReplyGateFailed {
		t.Fatalf("status = %q, want %q", report.Status, ReplyGateFailed)
	}
	if report.MatchedQuotes != 1 || report.FailedQuotes != 1 {
		t.Fatalf("matched=%d failed=%d", report.MatchedQuotes, report.FailedQuotes)
	}
}

func TestReplyGateFailsOnAmbiguousReferenceIDs(t *testing.T) {
	report := EvaluateReplyGate([]clawbot.DebugEntry{
		gateSendResult(t, "platform-1", "client-1"),
		gateSendResult(t, "platform-2", "client-2"),
		gateInbound(t, "reply-1", "platform-1", "platform-2"),
	}, resolvingGateRoute)

	if report.Status != ReplyGateFailed {
		t.Fatalf("status = %q, want %q", report.Status, ReplyGateFailed)
	}
	if report.FailedQuotes != 1 || report.MatchedQuotes != 0 {
		t.Fatalf("matched=%d failed=%d", report.MatchedQuotes, report.FailedQuotes)
	}
	if got := len(report.Quotes[0].ReferencedIDs); got != 2 {
		t.Fatalf("referenced IDs = %#v", report.Quotes[0].ReferencedIDs)
	}
}

func TestReplyGateIgnoresMessagesWithoutReference(t *testing.T) {
	report := EvaluateReplyGate([]clawbot.DebugEntry{
		gateSendResult(t, "platform-1", "client-1"),
		gateInbound(t, "reply-1"),
	}, resolvingGateRoute)

	if report.Status != ReplyGateAwaitingQuote {
		t.Fatalf("status = %q, want %q", report.Status, ReplyGateAwaitingQuote)
	}
	if len(report.Quotes) != 0 {
		t.Fatalf("quotes = %#v", report.Quotes)
	}
}

func TestReplyGateFailsWhenReferenceHasNoMessageID(t *testing.T) {
	report := EvaluateReplyGate([]clawbot.DebugEntry{
		gateSendResult(t, "platform-1", "client-1"),
		gateInboundReference(t, "reply-1", true),
	}, resolvingGateRoute)

	if report.Status != ReplyGateFailed {
		t.Fatalf("status = %q, want %q", report.Status, ReplyGateFailed)
	}
	if report.FailedQuotes != 1 || report.MatchedQuotes != 0 {
		t.Fatalf("matched=%d failed=%d", report.MatchedQuotes, report.FailedQuotes)
	}
	if len(report.Quotes) != 1 || report.Quotes[0].ReferenceError == "" {
		t.Fatalf("quotes = %#v", report.Quotes)
	}
}

func TestReplyGateDeduplicatesRepeatedSendRecords(t *testing.T) {
	send := gateSendResult(t, "platform-1", "client-1")
	report := EvaluateReplyGate([]clawbot.DebugEntry{
		send,
		send,
		gateEntry(t, clawbot.DebugOperationSendRequest, clawbot.DebugSendRequest{ClientID: "client-1"}),
	}, resolvingGateRoute)

	if len(report.Sends) != 1 {
		t.Fatalf("sends = %#v", report.Sends)
	}
}

func TestReplyGateFailsWhenRouteResolutionFails(t *testing.T) {
	report := EvaluateReplyGate([]clawbot.DebugEntry{
		gateSendResult(t, "platform-1", "client-1"),
		gateInbound(t, "reply-1", "platform-1"),
	}, func(string) (Route, error) {
		return Route{}, ErrRouteNotFound
	})

	if report.Status != ReplyGateFailed {
		t.Fatalf("status = %q, want %q", report.Status, ReplyGateFailed)
	}
	if report.RouteFailures != 1 {
		t.Fatalf("route failures = %d, want 1", report.RouteFailures)
	}
	detail := report.Quotes[0].RouteError
	if !strings.Contains(detail, ErrRouteNotFound.Error()) {
		t.Fatalf("route error = %q", detail)
	}
	if report.Quotes[0].RouteAgent != "" || report.Quotes[0].RouteSession != "" {
		t.Fatalf("quote = %#v", report.Quotes[0])
	}
}

func TestReplyGateFailsWhenRouteResolverIsUnavailable(t *testing.T) {
	report := EvaluateReplyGate([]clawbot.DebugEntry{
		gateSendResult(t, "platform-1", "client-1"),
		gateInbound(t, "reply-1", "platform-1"),
	}, nil)

	if report.Status != ReplyGateFailed {
		t.Fatalf("status = %q, want %q", report.Status, ReplyGateFailed)
	}
	if report.RouteFailures != 1 {
		t.Fatalf("route failures = %d, want 1", report.RouteFailures)
	}
	if report.Quotes[0].RouteError == "" {
		t.Fatal("missing route resolver error")
	}
}

func TestReplyGateReportsExpiredRoute(t *testing.T) {
	report := EvaluateReplyGate([]clawbot.DebugEntry{
		gateSendResult(t, "platform-1", "client-1"),
		gateInbound(t, "reply-1", "platform-1"),
	}, func(string) (Route, error) {
		return Route{}, ErrRouteExpired
	})

	if report.Status != ReplyGateFailed {
		t.Fatalf("status = %q, want %q", report.Status, ReplyGateFailed)
	}
	if detail := report.Quotes[0].RouteError; !strings.Contains(detail, ErrRouteExpired.Error()) {
		t.Fatalf("route error = %q", detail)
	}
	if report.MatchedQuotes != 1 || report.RouteFailures != 1 {
		t.Fatalf("matched=%d routeFailures=%d", report.MatchedQuotes, report.RouteFailures)
	}
}

func TestReplyGateForAccountPassesForBoundPrivateSample(t *testing.T) {
	const scope = "scope-current"
	report := EvaluateReplyGateForAccount([]clawbot.DebugEntry{
		gateScopedSendResult(t, scope, "platform-1", "client-1"),
		gateScopedInbound(t, scope, "reply-1", true, true, "platform-1"),
	}, scope, resolvingGateRoute)

	if report.Status != ReplyGatePassed {
		t.Fatalf("status = %q, want %q: %#v", report.Status, ReplyGatePassed, report)
	}
	if report.AccountScope != scope || report.IgnoredSends != 0 || report.IgnoredQuotes != 0 {
		t.Fatalf("report scope = %#v", report)
	}
}

func TestReplyGateForAccountIgnoresOtherAccountsAndNonPrivateSamples(t *testing.T) {
	const scope = "scope-current"
	report := EvaluateReplyGateForAccount([]clawbot.DebugEntry{
		gateScopedSendResult(t, "scope-other", "platform-other", "client-other"),
		gateScopedSendResult(t, scope, "platform-current", "client-current"),
		gateScopedInbound(t, "scope-other", "reply-other", true, true, "platform-other"),
		gateScopedInbound(t, scope, "reply-group", false, true, "platform-current"),
		gateScopedInbound(t, scope, "reply-stranger", true, false, "platform-current"),
	}, scope, resolvingGateRoute)

	if report.Status != ReplyGateAwaitingQuote {
		t.Fatalf("status = %q, want %q: %#v", report.Status, ReplyGateAwaitingQuote, report)
	}
	if report.IgnoredSends != 1 || report.IgnoredQuotes != 3 {
		t.Fatalf("ignored sends=%d quotes=%d", report.IgnoredSends, report.IgnoredQuotes)
	}
	if len(report.Sends) != 1 || report.Sends[0].MessageID != "platform-current" {
		t.Fatalf("sends = %#v", report.Sends)
	}
}

func TestReplyGatePassesUsingLocalRouteEvidence(t *testing.T) {
	const scope = "scope-current"
	report := EvaluateReplyGateForAccountWithSends(
		[]clawbot.DebugEntry{gateScopedInbound(t, scope, "reply-1", true, true, "platform-1")},
		scope,
		[]RecordedSend{{
			MessageID:    "platform-1",
			ClientID:     "client-1",
			Agent:        "codex",
			SessionID:    "thread-1",
			AccountScope: scope,
		}},
		resolvingGateRoute,
	)

	if report.Status != ReplyGatePassed {
		t.Fatalf("status = %q, want %q: %#v", report.Status, ReplyGatePassed, report)
	}
	if len(report.Sends) != 1 || report.Sends[0].Source != ReplyGateSendRoute {
		t.Fatalf("sends = %#v", report.Sends)
	}
	if report.Sends[0].Agent != "codex" || report.Sends[0].SessionID != "thread-1" {
		t.Fatalf("send route = %#v", report.Sends[0])
	}
}

func TestReplyGateIgnoresLocalRouteEvidenceFromAnotherAccount(t *testing.T) {
	const scope = "scope-current"
	report := EvaluateReplyGateForAccountWithSends(
		[]clawbot.DebugEntry{gateScopedInbound(t, scope, "reply-1", true, true, "platform-1")},
		scope,
		[]RecordedSend{{MessageID: "platform-1", AccountScope: "scope-other"}},
		resolvingGateRoute,
	)

	if report.Status != ReplyGateAwaitingSend {
		t.Fatalf("status = %q, want %q: %#v", report.Status, ReplyGateAwaitingSend, report)
	}
	if report.IgnoredSends != 1 || len(report.Sends) != 0 {
		t.Fatalf("report = %#v", report)
	}
}

func TestReplyGateMatchesLocalRouteByPlatformAndClientID(t *testing.T) {
	const scope = "scope-current"
	for _, test := range []struct {
		name      string
		reference string
		wantMatch ReplyGateMatch
	}{
		{name: "platform", reference: "platform-1", wantMatch: ReplyGateMatchPlatformID},
		{name: "client", reference: "client-1", wantMatch: ReplyGateMatchClientID},
	} {
		t.Run(test.name, func(t *testing.T) {
			report := EvaluateReplyGateForAccountWithSends(
				[]clawbot.DebugEntry{gateScopedInbound(t, scope, "reply-1", true, true, test.reference)},
				scope,
				[]RecordedSend{{
					MessageID:    "platform-1",
					ClientID:     "client-1",
					Agent:        "codex",
					SessionID:    "thread-1",
					AccountScope: scope,
				}}, resolvingGateRoute)

			if report.Status != ReplyGatePassed {
				t.Fatalf("status = %q, want %q: %#v", report.Status, ReplyGatePassed, report)
			}
			if report.Quotes[0].Match != test.wantMatch || report.Sends[0].Source != ReplyGateSendRoute {
				t.Fatalf("quote = %#v send = %#v", report.Quotes[0], report.Sends[0])
			}
		})
	}
}

func TestReplyGateFailsWhenLocalRouteIdentifierConflicts(t *testing.T) {
	const scope = "scope-current"
	report := EvaluateReplyGateForAccountWithSends(
		[]clawbot.DebugEntry{gateScopedInbound(t, scope, "reply-1", true, true, "shared-id")},
		scope,
		[]RecordedSend{{MessageID: "shared-id", AccountScope: scope, Agent: "codex", SessionID: "thread-1"}},
		func(string) (Route, error) { return Route{}, ErrRouteAmbiguous },
	)

	if report.Status != ReplyGateFailed || report.RouteFailures != 1 {
		t.Fatalf("report = %#v", report)
	}
}
