package notify

import "strings"

// HeartbeatState classifies the heartbeat block of one agent message.
type HeartbeatState string

const (
	HeartbeatNone     HeartbeatState = "none"
	HeartbeatNormal   HeartbeatState = "normal"
	HeartbeatAbnormal HeartbeatState = "abnormal"
	HeartbeatUnknown  HeartbeatState = "unknown"
)

// ProtocolResult is the outcome of parsing one raw agent message.
type ProtocolResult struct {
	Body      string
	Silent    bool
	Reason    string
	Heartbeat HeartbeatState
}

const (
	protocolTagHeartbeat  = "heartbeat"
	protocolTagAutomation = "automation_id"
	protocolTagDecision   = "decision"
	protocolTagMessage    = "message"

	decisionDoNotNotify = "DONT_NOTIFY"

	reasonHeartbeatNormal = "正常心跳，已静默"
	reasonDecisionSilent  = "消息标记为 DONT_NOTIFY，已静默"
	reasonEmptyBody       = "仅包含协议块，无正文，已静默"

	heartbeatAbnormalFallbackBody = "检测到异常心跳，但未提供详情。"
	heartbeatUnknownFallbackBody  = "无法识别心跳状态，但未提供详情。"
)

// protocolBlockTags lists the tags Agent-notify understands. Unknown tags stay
// in the body so unrelated markup is never lost.
var protocolBlockTags = map[string]struct{}{
	protocolTagHeartbeat:  {},
	protocolTagAutomation: {},
	protocolTagDecision:   {},
	protocolTagMessage:    {},
}

// ParseProtocolBlocks removes agent protocol markup from one raw message and
// decides whether the message must remain silent.
func ParseProtocolBlocks(text string) ProtocolResult {
	parser := protocolParser{source: strings.ReplaceAll(text, "\r\n", "\n")}
	nodes, _ := parser.parseNodes("")

	var body protocolBody
	heartbeat := HeartbeatNone
	decisionSilent := false

	for _, node := range nodes {
		if node.element == nil {
			body.append(node.text)
			continue
		}

		switch node.element.name {
		case protocolTagHeartbeat:
			state, message := classifyHeartbeat(node.element)
			if moreSevereHeartbeat(state, heartbeat) {
				heartbeat = state
				body = protocolBody{}
			}
			if state == heartbeat && state != HeartbeatNormal {
				body.append(message)
			}
		case protocolTagDecision:
			if strings.EqualFold(
				strings.TrimSpace(node.element.descendantText("")),
				decisionDoNotNotify,
			) {
				decisionSilent = true
			}
		case protocolTagMessage:
			body.append(node.element.descendantText(""))
		case protocolTagAutomation:
			// Automation identifiers are metadata and are intentionally hidden.
		}
	}

	result := ProtocolResult{
		Body:      body.string(),
		Heartbeat: heartbeat,
	}
	if result.Body == "" {
		switch heartbeat {
		case HeartbeatAbnormal:
			result.Body = heartbeatAbnormalFallbackBody
		case HeartbeatUnknown:
			result.Body = heartbeatUnknownFallbackBody
		}
	}
	switch {
	case decisionSilent:
		result.Silent = true
		result.Reason = reasonDecisionSilent
	case heartbeat == HeartbeatNormal:
		result.Silent = true
		result.Reason = reasonHeartbeatNormal
	case result.Body == "":
		result.Silent = true
		result.Reason = reasonEmptyBody
	}
	return result
}

func classifyHeartbeat(element *protocolElement) (HeartbeatState, string) {
	decision := strings.ToUpper(strings.TrimSpace(element.descendantText(protocolTagDecision)))
	if decision == decisionDoNotNotify {
		return HeartbeatNormal, ""
	}
	message := element.visibleText()
	if decision == "" {
		return HeartbeatUnknown, message
	}
	return HeartbeatAbnormal, message
}

func moreSevereHeartbeat(candidate, current HeartbeatState) bool {
	return heartbeatSeverity(candidate) > heartbeatSeverity(current)
}

func heartbeatSeverity(state HeartbeatState) int {
	switch state {
	case HeartbeatNone:
		return 0
	case HeartbeatNormal:
		return 1
	case HeartbeatUnknown:
		return 2
	case HeartbeatAbnormal:
		return 3
	default:
		return 2
	}
}

// protocolBody preserves paragraph boundaries between otherwise separate
// protocol nodes while dropping whitespace-only metadata gaps.
type protocolBody struct {
	parts []string
}

func (b *protocolBody) append(value string) {
	if value = strings.TrimSpace(value); value != "" {
		b.parts = append(b.parts, value)
	}
}

func (b protocolBody) string() string {
	return strings.TrimSpace(strings.Join(b.parts, "\n"))
}

type protocolNode struct {
	text    string
	element *protocolElement
}

type protocolElement struct {
	name     string
	children []protocolNode
}

func (e *protocolElement) descendantText(tag string) string {
	var text strings.Builder
	for _, child := range e.children {
		if child.element == nil {
			text.WriteString(child.text)
			continue
		}
		if tag == "" || strings.EqualFold(child.element.name, tag) {
			text.WriteString(child.element.descendantText(""))
		}
	}
	return text.String()
}

// visibleText returns the human-readable payload of a protocol element. Nested
// message blocks keep their content; metadata blocks are omitted.
func (e *protocolElement) visibleText() string {
	var text strings.Builder
	for _, child := range e.children {
		if child.element == nil {
			text.WriteString(child.text)
			continue
		}
		switch child.element.name {
		case protocolTagMessage:
			text.WriteString(child.element.descendantText(""))
		case protocolTagAutomation, protocolTagDecision:
			// Metadata without a display payload.
		default:
			text.WriteString(child.element.visibleText())
		}
	}
	return text.String()
}

// protocolParser is a tolerant scanner. Malformed or unknown markup is
// preserved as plain text rather than silently discarded.
type protocolParser struct {
	source string
	pos    int
}

func (p *protocolParser) parseNodes(stop string) ([]protocolNode, bool) {
	var nodes []protocolNode
	var text strings.Builder
	flush := func() {
		if text.Len() > 0 {
			nodes = append(nodes, protocolNode{text: text.String()})
			text.Reset()
		}
	}

	for p.pos < len(p.source) {
		next := strings.IndexByte(p.source[p.pos:], '<')
		if next < 0 {
			text.WriteString(p.source[p.pos:])
			p.pos = len(p.source)
			break
		}
		text.WriteString(p.source[p.pos : p.pos+next])
		p.pos += next

		end := strings.IndexByte(p.source[p.pos:], '>')
		if end < 0 {
			text.WriteString(p.source[p.pos:])
			p.pos = len(p.source)
			break
		}
		raw := p.source[p.pos : p.pos+end+1]
		name, closing, selfClosing := parseProtocolTag(raw)
		if name == "" {
			text.WriteString(raw)
			p.pos += end + 1
			continue
		}

		if closing {
			if stop != "" && strings.EqualFold(name, stop) {
				flush()
				p.pos += end + 1
				return nodes, true
			}
			text.WriteString(raw)
			p.pos += end + 1
			continue
		}

		p.pos += end + 1
		if selfClosing {
			flush()
			nodes = append(nodes, protocolNode{element: &protocolElement{
				name: name,
			}})
			continue
		}

		children, closed := p.parseNodes(name)
		if !closed {
			// Unbalanced markup stays visible instead of being swallowed.
			text.WriteString(raw)
			text.WriteString(untrustedText(children))
			continue
		}
		flush()
		nodes = append(nodes, protocolNode{element: &protocolElement{
			name:     name,
			children: children,
		}})
	}

	flush()
	return nodes, false
}

func untrustedText(nodes []protocolNode) string {
	var text strings.Builder
	for _, node := range nodes {
		if node.element == nil {
			text.WriteString(node.text)
			continue
		}
		switch node.element.name {
		case protocolTagDecision, protocolTagAutomation:
			// Malformed metadata is hidden, but never trusted as a decision.
		case protocolTagMessage:
			text.WriteString(node.element.descendantText(""))
		default:
			text.WriteString(node.element.visibleText())
		}
	}
	return text.String()
}

func parseProtocolTag(raw string) (name string, closing bool, selfClosing bool) {
	if len(raw) < 3 || raw[0] != '<' || raw[len(raw)-1] != '>' {
		return "", false, false
	}
	inner := strings.TrimSpace(raw[1 : len(raw)-1])
	if strings.HasPrefix(inner, "/") {
		closing = true
		inner = strings.TrimSpace(inner[1:])
	}
	if strings.HasSuffix(inner, "/") {
		selfClosing = true
		inner = strings.TrimSpace(strings.TrimSuffix(inner, "/"))
	}
	if index := strings.IndexAny(inner, " \t\r\n"); index >= 0 {
		inner = inner[:index]
	}
	if inner == "" || !isProtocolTagName(inner) {
		return "", closing, selfClosing
	}
	name = strings.ToLower(inner)
	if _, known := protocolBlockTags[name]; !known {
		return "", closing, selfClosing
	}
	return name, closing, selfClosing
}

func isProtocolTagName(name string) bool {
	for index, char := range name {
		switch {
		case char >= 'a' && char <= 'z', char >= 'A' && char <= 'Z':
		case index > 0 && char >= '0' && char <= '9':
		case index > 0 && (char == '_' || char == '-'):
		default:
			return false
		}
	}
	return name != ""
}
