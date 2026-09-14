package reply

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"strings"
	"time"
)

const (
	agentAPISendTimeout = 15 * time.Second
)

// AntigravityEndpoint 是一次 Antigravity 语言服务连接：只包含 agentapi 所需的
// 本机地址与 CSRF 令牌，二者都随桌面端每次启动而变化。
type AntigravityEndpoint struct {
	Address string
	Token   string
}

func (e AntigravityEndpoint) validate() error {
	if strings.TrimSpace(e.Address) == "" || strings.TrimSpace(e.Token) == "" {
		return errors.New("Antigravity 语言服务地址或令牌为空")
	}
	return nil
}

// EndpointResolver 返回一个可用于探测目标会话的语言服务端点。
type EndpointResolver func(ctx context.Context, sessionID string) (AntigravityEndpoint, error)

// AgentAPI 是 language_server.exe agentapi 暴露的两项能力。
type AgentAPI interface {
	ConversationExists(ctx context.Context, endpoint AntigravityEndpoint, sessionID string) (bool, error)
	SendMessage(ctx context.Context, endpoint AntigravityEndpoint, sessionID, text string) error
}

type agentAPIProcess struct {
	Binary string
	Runner ProcessRunner
}

func (p agentAPIProcess) ConversationExists(
	ctx context.Context,
	endpoint AntigravityEndpoint,
	sessionID string,
) (bool, error) {
	output, err := p.call(ctx, endpoint, "get-conversation-metadata", sessionID)
	if err != nil {
		return false, err
	}
	var response agentAPIResponse
	if err := json.Unmarshal(output, &response); err != nil {
		return false, fmt.Errorf("Antigravity 语言服务响应无法解析: %w", err)
	}
	if detail := strings.TrimSpace(response.Error); detail != "" {
		return false, fmt.Errorf("Antigravity 语言服务返回错误: %s", detail)
	}
	metadata := response.Response.ConversationMetadata.Metadata
	return metadata.RootConversationID == sessionID || metadata.ParentConversationID == sessionID, nil
}

func (p agentAPIProcess) SendMessage(
	ctx context.Context,
	endpoint AntigravityEndpoint,
	sessionID, text string,
) error {
	sendCtx, cancel := context.WithTimeout(ctx, agentAPISendTimeout)
	defer cancel()
	_, err := p.call(sendCtx, endpoint, "send-message", sessionID, text)
	return err
}

func (p agentAPIProcess) call(
	ctx context.Context,
	endpoint AntigravityEndpoint,
	args ...string,
) ([]byte, error) {
	if err := endpoint.validate(); err != nil {
		return nil, err
	}
	binary := strings.TrimSpace(p.Binary)
	if binary == "" {
		resolved, err := resolveAntigravityLanguageServerBinary()
		if err != nil {
			return nil, err
		}
		binary = resolved
	}
	runner := p.Runner
	if runner == nil {
		runner = hiddenProcessRunner{}
	}
	output, err := runner.Run(
		agentAPICommandContext(ctx, endpoint),
		binary,
		append([]string{"agentapi"}, args...)...,
	)
	if err != nil {
		return nil, err
	}
	return extractJSONObject(output)
}

// agentAPICommandContext 把地址与令牌通过环境变量传给官方 agentapi 客户端：
// 官方客户端自带 gRPC 与 CSRF 处理，无需在 Go 侧复刻私有协议。
func agentAPICommandContext(ctx context.Context, endpoint AntigravityEndpoint) context.Context {
	return withProcessEnv(ctx, map[string]string{
		"ANTIGRAVITY_LS_ADDRESS": endpoint.Address,
		"ANTIGRAVITY_CSRF_TOKEN": endpoint.Token,
	})
}

// AntigravityAgentAPISender 通过官方 agentapi 把回复写入精确的桌面会话。
type AntigravityAgentAPISender struct {
	Binary   string
	Resolver EndpointResolver
	API      AgentAPI
}

func (s AntigravityAgentAPISender) Send(ctx context.Context, sessionID, text string) error {
	sessionID = strings.TrimSpace(sessionID)
	text = strings.TrimSpace(text)
	if sessionID == "" {
		return errors.New("antigravity reply: session id is empty")
	}
	if text == "" {
		return errors.New("antigravity reply: message text is empty")
	}
	if err := ctx.Err(); err != nil {
		return err
	}

	resolver := s.Resolver
	if resolver == nil {
		resolver = newAntigravityResolver(s.Binary)
	}
	api := s.API
	if api == nil {
		api = agentAPIProcess{Binary: s.Binary}
	}
	endpoint, err := resolver(ctx, sessionID)
	if err != nil {
		return antigravityReplyError(err)
	}
	if err := api.SendMessage(ctx, endpoint, sessionID, text); err != nil {
		return antigravityReplyError(err)
	}
	return nil
}

// antigravityReplyError 只做分类与提示，不透传可能包含用户内容或令牌的原始输出。
func antigravityReplyError(err error) error {
	if err == nil {
		return nil
	}
	detail := compactError(err)
	normalized := strings.ToLower(detail)
	switch {
	case errors.Is(err, context.DeadlineExceeded):
		return errors.New("Antigravity 语言服务响应超时，请确认桌面端仍处于打开状态后重试")
	case errors.Is(err, context.Canceled):
		return err
	case strings.Contains(normalized, "csrf"), strings.Contains(normalized, "token"):
		return errors.New("Antigravity 语言服务令牌已失效，请在桌面端重新打开会话后重试")
	}
	return fmt.Errorf("Antigravity 引用回复失败: %s", detail)
}

type agentAPIResponse struct {
	Response struct {
		ConversationMetadata struct {
			Metadata struct {
				RootConversationID   string `json:"rootConversationId"`
				ParentConversationID string `json:"parentConversationId"`
			} `json:"metadata"`
		} `json:"conversationMetadata"`
	} `json:"response"`
	Error string `json:"error"`
}

// extractJSONObject 从 agentapi 输出中截取唯一的 JSON 对象，避免日志前缀干扰。
func extractJSONObject(output []byte) ([]byte, error) {
	start := -1
	end := -1
	for index, value := range output {
		if start < 0 {
			if value == '{' {
				start = index
			}
			continue
		}
		if value == '}' {
			end = index
		}
	}
	if start < 0 || end <= start {
		return nil, fmt.Errorf("antigravity reply: 语言服务未返回可解析的响应")
	}
	return output[start : end+1], nil
}
