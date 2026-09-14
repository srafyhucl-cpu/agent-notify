//go:build windows

package reply

import (
	"context"
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"strings"
)

const antigravityLanguageServerBinary = "language_server.exe"

const antigravityBinaryEnv = "AGENT_NOTIFY_ANTIGRAVITY_BIN"

var errAntigravityLanguageServerMissing = errors.New(
	"未找到 Antigravity 桌面端的语言服务：请确认 Antigravity 桌面端已安装并正在运行",
)

// newAntigravityResolver 让 sender 复用同一份发现逻辑，并沿用调用方指定的
// agentapi 程序，便于在不可用环境中替换实现。
func newAntigravityResolver(binary string) EndpointResolver {
	api := agentAPIProcess{Binary: binary}
	return func(ctx context.Context, sessionID string) (AntigravityEndpoint, error) {
		return resolveAntigravityEndpoint(ctx, sessionID, api)
	}
}

// resolveAntigravityEndpoint 定位当前正在运行的语言服务，并选定认识目标会话的端点。
// 语言服务的 PID、端口与 CSRF 令牌每次启动都会变化，因此不做任何缓存。
func resolveAntigravityEndpoint(
	ctx context.Context,
	sessionID string,
	api AgentAPI,
) (AntigravityEndpoint, error) {
	binary, err := resolveAntigravityLanguageServerBinary()
	if err != nil {
		return AntigravityEndpoint{}, err
	}
	candidates := antigravityEndpoints(languageServerProcesses(ctx, binary))
	if len(candidates) == 0 {
		return AntigravityEndpoint{}, errors.New(
			"未找到可用的 Antigravity 语言服务端口：请确认 Antigravity 桌面端仍在运行",
		)
	}
	return selectAntigravityEndpoint(ctx, api, candidates, sessionID)
}

func resolveAntigravityLanguageServerBinary() (string, error) {
	if override := strings.TrimSpace(os.Getenv(antigravityBinaryEnv)); override != "" {
		info, err := os.Stat(override)
		if err != nil {
			return "", fmt.Errorf("Antigravity 语言服务路径不可用: %w", err)
		}
		if info.IsDir() {
			return "", fmt.Errorf("Antigravity 语言服务路径不是文件: %s", override)
		}
		return override, nil
	}
	for _, candidate := range antigravityLanguageServerCandidates() {
		if info, err := os.Stat(candidate); err == nil && info.Mode().IsRegular() {
			return candidate, nil
		}
	}
	return "", errAntigravityLanguageServerMissing
}

func antigravityLanguageServerCandidates() []string {
	var candidates []string
	for _, root := range antigravityInstallRoots() {
		candidates = append(candidates, filepath.Join(root, "resources", "bin", antigravityLanguageServerBinary))
	}
	return candidates
}

func antigravityInstallRoots() []string {
	var roots []string
	for _, key := range []string{"LOCALAPPDATA", "APPDATA"} {
		root := strings.TrimSpace(os.Getenv(key))
		if root == "" {
			continue
		}
		roots = append(roots, filepath.Join(root, "Programs", "antigravity"))
	}
	return roots
}

// languageServerProcesses 只保留目标安装目录下的语言服务进程。命令行读不到时
// 直接跳过该进程，避免向来源不明的服务发送内容。
func languageServerProcesses(ctx context.Context, binary string) []agentProcess {
	var matched []agentProcess
	for _, process := range runningAgentProcesses(ctx, antigravityLanguageServerBinary) {
		if !sameAntigravityInstall(process.Executable, binary) {
			continue
		}
		if strings.TrimSpace(process.CommandLine) == "" {
			continue
		}
		matched = append(matched, process)
	}
	return matched
}

func sameAntigravityInstall(executable, binary string) bool {
	installRoot := filepath.Dir(filepath.Dir(filepath.Clean(executable)))
	expected := filepath.Dir(filepath.Dir(filepath.Clean(binary)))
	return strings.EqualFold(installRoot, expected)
}

// antigravityEndpoints 用命令行里的 CSRF 令牌与该进程监听的端口组合出候选端点。
// 其中既有 HTTP 端口也有 HTTPS 端口，具体哪个可用由探测决定。
func antigravityEndpoints(processes []agentProcess) []AntigravityEndpoint {
	var endpoints []AntigravityEndpoint
	seen := make(map[string]struct{})
	for _, process := range processes {
		token := parseCommandLineFlag(process.CommandLine, "--csrf_token")
		if token == "" {
			continue
		}
		for _, port := range listeningTCPPorts(process.PID) {
			address := fmt.Sprintf("127.0.0.1:%d", port)
			if _, exists := seen[address]; exists {
				continue
			}
			seen[address] = struct{}{}
			endpoints = append(endpoints, AntigravityEndpoint{Address: address, Token: token})
		}
	}
	return endpoints
}

// selectAntigravityEndpoint 探测候选端点，直到找到能读到目标会话的实例。
func selectAntigravityEndpoint(
	ctx context.Context,
	api AgentAPI,
	candidates []AntigravityEndpoint,
	sessionID string,
) (AntigravityEndpoint, error) {
	if len(candidates) == 0 {
		return AntigravityEndpoint{}, errors.New("没有可探测的 Antigravity 语言服务端点")
	}
	var lastErr error
	for _, candidate := range candidates {
		if err := ctx.Err(); err != nil {
			return AntigravityEndpoint{}, err
		}
		exists, err := api.ConversationExists(ctx, candidate, sessionID)
		if err != nil {
			lastErr = err
			continue
		}
		if exists {
			return candidate, nil
		}
	}
	if lastErr != nil {
		return AntigravityEndpoint{}, lastErr
	}
	return AntigravityEndpoint{}, fmt.Errorf(
		"Antigravity 会话 %s 当前不可用：请确认该会话仍存在于桌面端",
		sessionID,
	)
}
