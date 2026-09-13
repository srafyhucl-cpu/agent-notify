package reply

import (
	"strings"

	"github.com/srafyhucl-cpu/agent-notify/internal/clawbot"
)

// ReplyGateStatus summarizes whether quoted replies can be correlated safely.
type ReplyGateStatus string

const (
	ReplyGateAwaitingSend  ReplyGateStatus = "awaiting-send"
	ReplyGateAwaitingQuote ReplyGateStatus = "awaiting-quote"
	ReplyGatePassed        ReplyGateStatus = "passed"
	ReplyGateFailed        ReplyGateStatus = "failed"
)

// ReplyGateMatch describes which recorded identifier a quoted message matched.
type ReplyGateMatch string

const (
	ReplyGateMatchPlatformID ReplyGateMatch = "platform-id"
	ReplyGateMatchClientID   ReplyGateMatch = "client-id"
	ReplyGateMatchNone       ReplyGateMatch = "none"
)

// ReplyGateSend is one recorded outbound notification.
type ReplyGateSend struct {
	MessageID string              `json:"messageID,omitempty"`
	ClientID  string              `json:"clientID,omitempty"`
	Source    ReplyGateSendSource `json:"source"`
	Agent     string              `json:"agent,omitempty"`
	SessionID string              `json:"sessionID,omitempty"`
}

// ReplyGateSendSource identifies where one outbound-message record came from.
type ReplyGateSendSource string

const (
	ReplyGateSendDebug ReplyGateSendSource = "debug-send"
	ReplyGateSendRoute ReplyGateSendSource = "local-route"
)

// RecordedSend is one scoped outbound-message record supplied by either
// diagnostics or the local route store.
type RecordedSend struct {
	MessageID    string `json:"messageID,omitempty"`
	ClientID     string `json:"clientID,omitempty"`
	Agent        string `json:"agent,omitempty"`
	SessionID    string `json:"sessionID,omitempty"`
	AccountScope string `json:"accountScope,omitempty"`
}

// ReplyGateQuote is one observed quoted reply and its routing outcome.
type ReplyGateQuote struct {
	MessageID      string         `json:"messageID,omitempty"`
	ReferencedIDs  []string       `json:"referencedIDs"`
	Match          ReplyGateMatch `json:"match"`
	MatchedID      string         `json:"matchedID,omitempty"`
	ReferenceError string         `json:"referenceError,omitempty"`
	RouteAgent     string         `json:"routeAgent,omitempty"`
	RouteSession   string         `json:"routeSession,omitempty"`
	RouteError     string         `json:"routeError,omitempty"`
}

// ReplyGateReport is the evidence for the pre-release ID correlation gate.
type ReplyGateReport struct {
	Status        ReplyGateStatus  `json:"status"`
	Sends         []ReplyGateSend  `json:"sends"`
	Quotes        []ReplyGateQuote `json:"quotes"`
	MatchedQuotes int              `json:"matchedQuotes"`
	FailedQuotes  int              `json:"failedQuotes"`
	RouteFailures int              `json:"routeFailures"`
	AccountScope  string           `json:"accountScope,omitempty"`
	IgnoredSends  int              `json:"ignoredSends,omitempty"`
	IgnoredQuotes int              `json:"ignoredQuotes,omitempty"`
}

type replyGateSendKey struct {
	MessageID string
	ClientID  string
}

// RouteResolver resolves one quoted identifier exactly like the dispatcher does.
type RouteResolver func(referencedID string) (Route, error)

// EvaluateReplyGate correlates sanitized protocol diagnostics. It reports
// success when every observed quote matches a recorded outbound message
// and resolves to a persisted route.
func EvaluateReplyGate(entries []clawbot.DebugEntry, resolve RouteResolver) ReplyGateReport {
	return evaluateReplyGate(entries, "", nil, resolve)
}

// EvaluateReplyGateForAccount evaluates only diagnostics emitted for the
// current ClawBot account and private conversation with the bound user.
func EvaluateReplyGateForAccount(entries []clawbot.DebugEntry, accountScope string, resolve RouteResolver) ReplyGateReport {
	return evaluateReplyGate(entries, strings.TrimSpace(accountScope), nil, resolve)
}

// EvaluateReplyGateForAccountWithSends also accepts scoped outbound evidence
// from durable local routes. Records without a matching account scope are
// ignored, so re-login boundaries cannot reuse route evidence.
func EvaluateReplyGateForAccountWithSends(
	entries []clawbot.DebugEntry,
	accountScope string,
	recorded []RecordedSend,
	resolve RouteResolver,
) ReplyGateReport {
	return evaluateReplyGate(entries, strings.TrimSpace(accountScope), recorded, resolve)
}

func evaluateReplyGate(
	entries []clawbot.DebugEntry,
	accountScope string,
	recorded []RecordedSend,
	resolve RouteResolver,
) ReplyGateReport {
	report := ReplyGateReport{
		Sends:        make([]ReplyGateSend, 0, len(entries)+len(recorded)),
		Quotes:       make([]ReplyGateQuote, 0, len(entries)),
		AccountScope: accountScope,
	}
	platformIDs := make(map[string]struct{})
	clientIDs := make(map[string]struct{})
	sentPairs := make(map[replyGateSendKey]struct{})

	recordSend := func(record RecordedSend, source ReplyGateSendSource) {
		record = record.normalized()
		if record.MessageID == "" && record.ClientID == "" {
			return
		}
		if accountScope != "" && record.AccountScope != accountScope {
			report.IgnoredSends++
			return
		}
		key := replyGateSendKey{MessageID: record.MessageID, ClientID: record.ClientID}
		if _, duplicate := sentPairs[key]; duplicate {
			return
		}
		sentPairs[key] = struct{}{}
		report.Sends = append(report.Sends, ReplyGateSend{
			MessageID: record.MessageID,
			ClientID:  record.ClientID,
			Source:    source,
			Agent:     record.Agent,
			SessionID: record.SessionID,
		})
		if record.MessageID != "" {
			platformIDs[record.MessageID] = struct{}{}
		}
		if record.ClientID != "" {
			clientIDs[record.ClientID] = struct{}{}
		}
	}

	for _, entry := range entries {
		result, ok := entry.SendResult()
		if !ok {
			continue
		}
		recordSend(RecordedSend{
			MessageID:    result.MessageID,
			ClientID:     result.ClientID,
			AccountScope: result.AccountScope,
		}, ReplyGateSendDebug)
	}
	for _, record := range recorded {
		recordSend(record, ReplyGateSendRoute)
	}

	for _, entry := range entries {
		references, ok := entry.InboundReferences()
		if !ok {
			continue
		}
		for _, reference := range references {
			if len(reference.ReferencedMessageIDs) == 0 && !reference.HasReference {
				continue
			}
			if accountScope != "" && (strings.TrimSpace(reference.AccountScope) != accountScope || !reference.Private || !reference.BoundSender) {
				report.IgnoredQuotes++
				continue
			}
			quote := ReplyGateQuote{
				MessageID:     reference.MessageID,
				ReferencedIDs: reference.ReferencedMessageIDs,
				Match:         ReplyGateMatchNone,
			}
			if len(reference.ReferencedMessageIDs) == 0 {
				quote.ReferenceError = "reference marker present but no referenced message ID"
				report.FailedQuotes++
				report.Quotes = append(report.Quotes, quote)
				continue
			}
			if len(reference.ReferencedMessageIDs) == 1 {
				quoted := reference.ReferencedMessageIDs[0]
				switch {
				case containsID(platformIDs, quoted):
					quote.Match = ReplyGateMatchPlatformID
					quote.MatchedID = quoted
				case containsID(clientIDs, quoted):
					quote.Match = ReplyGateMatchClientID
					quote.MatchedID = quoted
				}
			}
			if quote.MatchedID != "" {
				report.MatchedQuotes++
				if resolve == nil {
					quote.RouteError = "route resolver unavailable"
					report.RouteFailures++
				} else {
					resolveRoute(resolve, &quote)
					if quote.RouteError != "" {
						report.RouteFailures++
					}
				}
			} else {
				report.FailedQuotes++
			}
			report.Quotes = append(report.Quotes, quote)
		}
	}

	report.Status = replyGateStatus(report)
	return report
}

func (r RecordedSend) normalized() RecordedSend {
	r.MessageID = strings.TrimSpace(r.MessageID)
	r.ClientID = strings.TrimSpace(r.ClientID)
	r.Agent = strings.TrimSpace(r.Agent)
	r.SessionID = strings.TrimSpace(r.SessionID)
	r.AccountScope = strings.TrimSpace(r.AccountScope)
	return r
}

func resolveRoute(resolve RouteResolver, quote *ReplyGateQuote) {
	route, err := resolve(quote.MatchedID)
	if err != nil {
		quote.RouteError = err.Error()
		return
	}
	quote.RouteAgent = strings.TrimSpace(route.Agent)
	quote.RouteSession = strings.TrimSpace(route.SessionID)
}

func containsID(ids map[string]struct{}, id string) bool {
	_, ok := ids[strings.TrimSpace(id)]
	return ok
}

func replyGateStatus(report ReplyGateReport) ReplyGateStatus {
	switch {
	case len(report.Sends) == 0:
		return ReplyGateAwaitingSend
	case len(report.Quotes) == 0:
		return ReplyGateAwaitingQuote
	case report.FailedQuotes > 0 || report.RouteFailures > 0:
		return ReplyGateFailed
	default:
		return ReplyGatePassed
	}
}
