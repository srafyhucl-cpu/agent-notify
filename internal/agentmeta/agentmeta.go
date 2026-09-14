// Package agentmeta contains the shared identity and rendering metadata for
// agents supported by Agent-notify. It intentionally has no project imports so
// notify, reply, and UI code can all consume the same definitions.
package agentmeta

import "strings"

const (
	OpenCode    = "opencode"
	Codex       = "codex"
	Antigravity = "antigravity"
	Devin       = "devin"
)

// Descriptor describes one supported agent's notification identity.
// Replyable is true only when notifications can carry an exact resume target.
type Descriptor struct {
	ID           string
	DisplayName  string
	TitlePrefix  string
	DefaultTitle string
	FooterLabel  string
	Replyable    bool
}

var descriptors = []Descriptor{
	{
		ID:           OpenCode,
		DisplayName:  "OpenCode",
		TitlePrefix:  "【opencode】",
		DefaultTitle: "opencode会话",
		FooterLabel:  "OpenCode",
		Replyable:    true,
	},
	{
		ID:           Codex,
		DisplayName:  "Codex",
		TitlePrefix:  "【codex】",
		DefaultTitle: "跑完了",
		FooterLabel:  "Codex",
		Replyable:    true,
	},
	{
		ID:           Antigravity,
		DisplayName:  "Antigravity",
		TitlePrefix:  "【antigravity】",
		DefaultTitle: "跑完了",
		FooterLabel:  "Antigravity",
		Replyable:    true,
	},
	{
		ID:           Devin,
		DisplayName:  "Devin",
		TitlePrefix:  "【devin】",
		DefaultTitle: "跑完了",
		FooterLabel:  "Devin",
		Replyable:    true,
	},
}

// Lookup returns metadata for a supported agent. Input is case-insensitive and
// surrounding whitespace is ignored.
func Lookup(agent string) (Descriptor, bool) {
	agent = strings.ToLower(strings.TrimSpace(agent))
	for _, descriptor := range descriptors {
		if descriptor.ID == agent {
			return descriptor, true
		}
	}
	return Descriptor{}, false
}

// All returns a copy of the supported agents in stable display order.
func All() []Descriptor {
	result := make([]Descriptor, len(descriptors))
	copy(result, descriptors)
	return result
}

// IsReplyable reports whether notifications from an agent carry a stable
// session target that can be resumed by the reply dispatcher.
func IsReplyable(agent string) bool {
	descriptor, ok := Lookup(agent)
	return ok && descriptor.Replyable
}
