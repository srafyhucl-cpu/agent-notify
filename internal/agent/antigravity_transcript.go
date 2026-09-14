package agent

import (
	"bufio"
	"bytes"
	"encoding/json"
	"io"
	"os"
	"strings"
)

const (
	antigravityTranscriptTailBytes = 512 * 1024
	antigravitySummaryMaxRunes     = 12000
	antigravityMaxLineBytes        = 1024 * 1024
)

type transcriptCandidate struct {
	text  string
	score int
}

// readAntigravityTranscriptSummary tolerates schema changes in Antigravity's
// private transcript format. The hook still succeeds when no useful assistant
// text can be identified.
func readAntigravityTranscriptSummary(path string) string {
	path = strings.TrimSpace(path)
	if path == "" {
		return ""
	}
	data, err := readFileTail(path, antigravityTranscriptTailBytes)
	if err != nil {
		return ""
	}
	return extractAntigravityTranscriptSummary(data)
}

func readFileTail(path string, limit int64) ([]byte, error) {
	file, err := os.Open(path)
	if err != nil {
		return nil, err
	}
	defer file.Close()

	info, err := file.Stat()
	if err != nil {
		return nil, err
	}
	offset := info.Size() - limit
	if offset < 0 {
		offset = 0
	}
	if _, err := file.Seek(offset, io.SeekStart); err != nil {
		return nil, err
	}
	return io.ReadAll(io.LimitReader(file, limit))
}

func extractAntigravityTranscriptSummary(data []byte) string {
	scanner := bufio.NewScanner(bytes.NewReader(data))
	scanner.Buffer(make([]byte, 64*1024), antigravityMaxLineBytes)
	best := transcriptCandidate{}
	for scanner.Scan() {
		line := bytes.TrimSpace(scanner.Bytes())
		if len(line) == 0 {
			continue
		}
		var node any
		if err := json.Unmarshal(line, &node); err != nil {
			continue
		}
		candidate := transcriptCandidateFromNode(node, "")
		if candidate.text != "" && candidate.score >= best.score {
			best = candidate
		}
	}
	return truncateTranscriptSummary(best.text, antigravitySummaryMaxRunes)
}

func transcriptCandidateFromNode(node any, inheritedRole string) transcriptCandidate {
	switch value := node.(type) {
	case []any:
		best := transcriptCandidate{}
		for _, item := range value {
			candidate := transcriptCandidateFromNode(item, inheritedRole)
			if candidate.text != "" && candidate.score >= best.score {
				best = candidate
			}
		}
		return best
	case map[string]any:
		role := transcriptRole(firstStringField(value, "role", "speaker", "author", "source"))
		kind := transcriptRole(firstStringField(value, "type", "kind", "event"))
		contextRole := inheritedRole
		if role != "" {
			contextRole = role
		}
		if transcriptRoleExcluded(contextRole) || transcriptRoleExcluded(kind) {
			return transcriptCandidate{}
		}

		assistantContext := transcriptRoleAssistant(contextRole) || transcriptRoleAssistant(kind)
		best := transcriptCandidate{}
		for _, field := range []struct {
			name  string
			score int
		}{
			{name: "last_assistant_message", score: 500},
			{name: "assistant_message", score: 500},
			{name: "response", score: 80},
			{name: "output", score: 70},
			{name: "content", score: 60},
			{name: "message", score: 50},
			{name: "text", score: 40},
		} {
			raw, ok := value[field.name]
			if !ok {
				continue
			}
			text := flattenTranscriptText(raw)
			if text == "" {
				continue
			}
			score := field.score
			if assistantContext {
				score += 200
			}
			candidate := transcriptCandidate{text: text, score: score}
			if candidate.score >= best.score {
				best = candidate
			}
		}

		for key, raw := range value {
			if isTranscriptTextKey(key) {
				continue
			}
			candidate := transcriptCandidateFromNode(raw, contextRole)
			if candidate.text != "" && candidate.score > best.score {
				best = candidate
			}
		}
		return best
	default:
		return transcriptCandidate{}
	}
}

func flattenTranscriptText(value any) string {
	var parts []string
	var appendValue func(any, bool)
	appendValue = func(current any, nested bool) {
		switch typed := current.(type) {
		case string:
			if text := strings.TrimSpace(typed); text != "" {
				parts = append(parts, text)
			}
		case []any:
			for _, item := range typed {
				appendValue(item, nested)
			}
		case map[string]any:
			for key, item := range typed {
				lower := strings.ToLower(strings.TrimSpace(key))
				switch lower {
				case "text", "content", "parts", "value", "message", "response", "output":
					appendValue(item, true)
				case "thought", "thinking", "tool_calls", "function_call", "args", "arguments", "metadata":
					continue
				default:
					if !nested && lower == "type" {
						continue
					}
				}
			}
		}
	}
	appendValue(value, false)
	return strings.TrimSpace(strings.Join(parts, "\n"))
}

func firstStringField(value map[string]any, keys ...string) string {
	for _, key := range keys {
		if text, ok := value[key].(string); ok && strings.TrimSpace(text) != "" {
			return text
		}
	}
	return ""
}

func transcriptRole(value string) string {
	value = strings.ToLower(strings.TrimSpace(value))
	value = strings.NewReplacer("-", "", "_", "", " ", "").Replace(value)
	return value
}

func transcriptRoleAssistant(role string) bool {
	return strings.Contains(role, "assistant") || role == "model" ||
		strings.Contains(role, "plannerresponse") || strings.Contains(role, "agentresponse")
}

func transcriptRoleExcluded(role string) bool {
	if role == "" {
		return false
	}
	return strings.Contains(role, "user") || strings.Contains(role, "human") ||
		strings.Contains(role, "tool") || strings.Contains(role, "function") || strings.Contains(role, "system")
}

func isTranscriptTextKey(key string) bool {
	switch strings.ToLower(strings.TrimSpace(key)) {
	case "last_assistant_message", "assistant_message", "response", "output", "content", "message", "text":
		return true
	default:
		return false
	}
}

func truncateTranscriptSummary(text string, limit int) string {
	text = strings.TrimSpace(text)
	if limit <= 0 {
		return text
	}
	runes := []rune(text)
	if len(runes) <= limit {
		return text
	}
	return strings.TrimSpace(string(runes[:limit])) + "…"
}
