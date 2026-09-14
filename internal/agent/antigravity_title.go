package agent

import (
	"bufio"
	"bytes"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"os"
	"path/filepath"
	"regexp"
	"strconv"
	"strings"
)

const (
	antigravityAnnotationMaxBytes   = 1 << 20
	antigravityTranscriptTitleBytes = 256 * 1024
	antigravityTitleMaxRunes        = 80
)

const (
	antigravityTitleSourceAnnotation = "annotations"
	antigravityTitleSourceTranscript = "transcript"
	antigravityTitleSourceFallback   = "fallback"
)

const (
	antigravityTitlePromptWarning   = "未能读取 Antigravity 会话标题，已使用首条用户请求作为标题。"
	antigravityTitleFallbackWarning = "未能读取 Antigravity 会话标题，已使用默认标题。"
)

var antigravityTitleFieldPattern = regexp.MustCompile(`(?:^|\s)title\s*:\s*("(?:\\.|[^"\\])*")`)

type antigravityTitleResolution struct {
	Title   string
	Source  string
	Warning string
}

func resolveAntigravityTitle(annotationsDir, conversationID, transcriptPath string) antigravityTitleResolution {
	title, err := readAntigravityConversationTitle(annotationsDir, conversationID)
	if err == nil {
		return antigravityTitleResolution{Title: title, Source: antigravityTitleSourceAnnotation}
	}

	if prompt := readAntigravityTranscriptTitle(transcriptPath); prompt != "" {
		return antigravityTitleResolution{
			Title:   prompt,
			Source:  antigravityTitleSourceTranscript,
			Warning: antigravityTitlePromptWarning,
		}
	}
	return antigravityTitleResolution{
		Source:  antigravityTitleSourceFallback,
		Warning: antigravityTitleFallbackWarning,
	}
}

func readAntigravityConversationTitle(annotationsDir, conversationID string) (string, error) {
	annotationsDir = strings.TrimSpace(annotationsDir)
	conversationID = strings.TrimSpace(conversationID)
	if annotationsDir == "" {
		return "", errors.New("Antigravity annotations directory is empty")
	}
	if conversationID == "" || filepath.Base(conversationID) != conversationID || conversationID == "." {
		return "", errors.New("Antigravity conversation id is unsafe")
	}

	path := filepath.Join(annotationsDir, conversationID+".pbtxt")
	file, err := os.Open(path)
	if err != nil {
		return "", fmt.Errorf("open Antigravity annotation: %w", err)
	}
	defer file.Close()

	data, err := io.ReadAll(io.LimitReader(file, antigravityAnnotationMaxBytes+1))
	if err != nil {
		return "", fmt.Errorf("read Antigravity annotation: %w", err)
	}
	if len(data) > antigravityAnnotationMaxBytes {
		return "", fmt.Errorf("Antigravity annotation exceeds %d bytes", antigravityAnnotationMaxBytes)
	}
	return parseAntigravityAnnotationTitle(data)
}

func parseAntigravityAnnotationTitle(data []byte) (string, error) {
	match := antigravityTitleFieldPattern.FindSubmatch(data)
	if len(match) != 2 {
		return "", errors.New("Antigravity annotation has no title field")
	}

	title, err := strconv.Unquote(string(match[1]))
	if err != nil {
		return "", fmt.Errorf("decode Antigravity annotation title: %w", err)
	}
	title = strings.Join(strings.Fields(strings.TrimSpace(title)), " ")
	if title == "" {
		return "", errors.New("Antigravity annotation title is empty")
	}
	return truncateTranscriptSummary(title, antigravityTitleMaxRunes), nil
}

func readAntigravityTranscriptTitle(path string) string {
	path = strings.TrimSpace(path)
	if path == "" {
		return ""
	}
	file, err := os.Open(path)
	if err != nil {
		return ""
	}
	defer file.Close()

	scanner := bufio.NewScanner(io.LimitReader(file, antigravityTranscriptTitleBytes))
	scanner.Buffer(make([]byte, 64*1024), antigravityMaxLineBytes)
	for scanner.Scan() {
		line := bytes.TrimSpace(scanner.Bytes())
		if len(line) == 0 {
			continue
		}
		var node map[string]any
		if err := json.Unmarshal(line, &node); err != nil {
			continue
		}
		if !strings.EqualFold(strings.TrimSpace(firstStringField(node, "type", "kind", "event")), "USER_INPUT") {
			continue
		}
		if title := cleanAntigravityPromptTitle(firstStringField(node, "content", "text", "message")); title != "" {
			return title
		}
	}
	return ""
}

func cleanAntigravityPromptTitle(content string) string {
	content = strings.TrimSpace(content)
	if content == "" {
		return ""
	}
	if start := strings.Index(content, "<USER_REQUEST>"); start >= 0 {
		content = content[start+len("<USER_REQUEST>"):]
	}
	if end := strings.Index(content, "</USER_REQUEST>"); end >= 0 {
		content = content[:end]
	}
	if end := strings.Index(content, "<ADDITIONAL_METADATA>"); end >= 0 {
		content = content[:end]
	}
	content = strings.Join(strings.Fields(strings.TrimSpace(content)), " ")
	return truncateTranscriptSummary(content, antigravityTitleMaxRunes)
}
