// Package integration 检测各 Agent 是否真正接入了 Agent-notify。
// 这里只读取配置、安装文件和心跳，不启动或重启任何 Agent。
package integration

import (
	"encoding/json"
	"fmt"
	"os"
	"path/filepath"
	"regexp"
	"strconv"
	"strings"
	"time"

	"github.com/srafyhucl-cpu/agent-notify/internal/agentmeta"
	"github.com/srafyhucl-cpu/agent-notify/internal/config"
)

const (
	heartbeatMaxAge     = 30 * time.Second
	heartbeatFutureSkew = 5 * time.Second

	// RepairCodexWatch 表示可由 Agent-notify 安全恢复的 Codex notify 配置。
	RepairCodexWatch = "codex_watch"
)

type State string

const (
	StateNotDetected    State = "not_detected"
	StatePendingRestart State = "pending_restart"
	StateError          State = "error"
	StateConnected      State = "connected"
)

type Status struct {
	Agent        string `json:"agent"`
	Name         string `json:"name"`
	State        State  `json:"state"`
	Enabled      bool   `json:"enabled"`
	InUse        bool   `json:"inUse"`
	Detail       string `json:"detail"`
	Action       string `json:"action,omitempty"`
	Repair       string `json:"repair,omitempty"`
	Fixable      bool   `json:"fixable"`
	NeedsRestart bool   `json:"needsRestart"`
}

type Options struct {
	Paths      config.Paths
	Executable string
	Enabled    map[string]bool
	Now        time.Time
}

func CheckAll(options Options) []Status {
	now := options.Now
	if now.IsZero() {
		now = time.Now()
	}
	return []Status{
		checkOpenCode(options.Paths, options.Executable, enabled(options.Enabled, agentmeta.OpenCode), now),
		checkCodex(options.Paths, enabled(options.Enabled, agentmeta.Codex)),
		checkAntigravity(options.Paths, enabled(options.Enabled, agentmeta.Antigravity)),
		checkDevin(options.Paths, enabled(options.Enabled, agentmeta.Devin), now),
	}
}

func (s Status) Label() string {
	if !s.Enabled {
		return "已暂停"
	}
	switch s.State {
	case StateConnected:
		return "已接入"
	case StatePendingRestart:
		return "待重启"
	case StateError:
		return "接入异常"
	default:
		return "未接入"
	}
}

func enabled(values map[string]bool, agentID string) bool {
	if values == nil {
		return true
	}
	value, ok := values[agentID]
	return !ok || value
}

func checkOpenCode(paths config.Paths, executable string, enabled bool, now time.Time) Status {
	status := newStatus(agentmeta.OpenCode, "OpenCode", enabled)
	data, err := os.ReadFile(paths.PluginFile)
	if err != nil {
		if os.IsNotExist(err) {
			return status.notDetected("未安装 OpenCode 插件", "请重新运行 install.ps1")
		}
		return status.failure("读取 OpenCode 插件失败："+err.Error(), "")
	}
	content := string(data)
	if !strings.Contains(content, "session.execution.succeeded") {
		return status.failure("OpenCode 插件版本过旧，未监听任务完成事件", "请重新运行 install.ps1")
	}
	binary := bakedPluginBinary(content)
	if binary == "" {
		binary = strings.TrimSpace(executable)
	}
	if binary == "" {
		binary = defaultAgentNotifyBinary()
	}
	if binary == "" || !fileExists(binary) {
		return status.failure("OpenCode 插件指向的 agent-notify.exe 不存在", "请重新运行 install.ps1")
	}

	heartbeat := inspectHeartbeat(paths.OpenCodeReplyDir, now)
	switch {
	case heartbeat.fresh:
		return status.connected("插件已加载，任务完成通知可推送", "")
	case heartbeat.invalid:
		return status.failure("OpenCode 插件心跳无效", "请完全退出并重启 OpenCode")
	case heartbeat.present:
		return status.pending("插件已安装，但当前 OpenCode 进程尚未加载", "请完全退出并重启 OpenCode")
	default:
		return status.pending("插件已安装，等待 OpenCode 加载", "请完全退出并重启 OpenCode")
	}
}

func checkCodex(paths config.Paths, enabled bool) Status {
	status := newStatus(agentmeta.Codex, "Codex", enabled)
	data, err := os.ReadFile(paths.CodexConfig)
	if err != nil {
		if os.IsNotExist(err) {
			return status.notDetected("未发现 Codex config.toml", "使用 Codex 后重新运行 install.ps1")
		}
		return status.failure("读取 Codex 配置失败："+err.Error(), "")
	}
	line := notifyLine(string(data))
	if strings.TrimSpace(line) == "" {
		return status.notDetected("Codex 未配置 notify", "请重新运行 install.ps1")
	}
	target := firstCommandPath(line)
	if target == "" {
		return status.failure("Codex notify 未包含有效的 agent-notify.exe 路径", "请运行 agent-notify watch")
	}
	lowerTarget := strings.ToLower(target)
	if !strings.Contains(lowerTarget, "agent-notify") {
		status.InUse = true
		if strings.Contains(lowerTarget, "codex-computer-use.exe") {
			return status.failureWithRepair("Codex notify 仍指向 codex-computer-use.exe", "点击检查修复可安全恢复", RepairCodexWatch)
		}
		return status.notDetected("Codex notify 使用其他程序，未接入 Agent-notify", "如不再需要原 notify，请重新运行 install.ps1")
	}
	status.InUse = true
	if !fileExists(target) {
		return status.failure("Codex notify 指向的 agent-notify.exe 不存在："+target, "请重新运行 install.ps1")
	}
	return status.connected("Codex notify 已配置并指向有效程序", "")
}

func checkAntigravity(paths config.Paths, enabled bool) Status {
	status := newStatus(agentmeta.Antigravity, "Antigravity", enabled)
	data, err := os.ReadFile(paths.AntigravityHooks)
	if err != nil {
		if os.IsNotExist(err) {
			return status.notDetected("未发现 Antigravity hooks.json", "安装 Antigravity 后重新运行 install.ps1")
		}
		return status.failure("读取 Antigravity Hook 失败："+err.Error(), "")
	}
	root, err := decodeObject(data)
	if err != nil {
		return status.failure("Antigravity hooks.json 格式无效："+err.Error(), "请修复 JSON 后重新运行 install.ps1")
	}
	command := antigravityCommand(root)
	if command == "" {
		return status.notDetected("Antigravity 未配置 Agent-notify Stop Hook", "请重新运行 install.ps1")
	}
	status.InUse = true
	if !strings.Contains(strings.ToLower(command), "antigravity stop") {
		return status.failure("Antigravity Hook 命令无效", "请重新运行 install.ps1")
	}
	target := firstCommandPath(command)
	if target == "" {
		return status.failure("Antigravity Hook 未包含有效启动器", "请重新运行 install.ps1")
	}
	if strings.EqualFold(filepath.Base(target), "agent-notify-hook.cmd") {
		launcher := resolveRelativePath(target, filepath.Dir(paths.AntigravityHooks))
		content, err := os.ReadFile(launcher)
		if err != nil {
			return status.failure("Antigravity 启动器不存在："+launcher, "请重新运行 install.ps1")
		}
		if !strings.Contains(string(content), "agent-notify-antigravity-launcher") {
			return status.failure("Antigravity 启动器不是 Agent-notify 创建的文件", "请重新运行 install.ps1")
		}
		target = firstCommandPath(string(content))
	}
	if target == "" || !fileExists(target) {
		return status.failure("Antigravity Hook 指向的 agent-notify.exe 不存在", "请重新运行 install.ps1")
	}
	return status.connected("Hook 配置有效；Antigravity 无加载心跳，需由真实任务完成验证", "")
}

func checkDevin(paths config.Paths, enabled bool, now time.Time) Status {
	status := newStatus(agentmeta.Devin, "Devin", enabled)
	data, err := os.ReadFile(paths.DevinConfig)
	if err != nil {
		if os.IsNotExist(err) {
			return status.notDetected("未发现 Devin config.json", "安装 Devin 后重新运行 install.ps1")
		}
		return status.failure("读取 Devin Hook 失败："+err.Error(), "")
	}
	root, err := decodeObject(data)
	if err != nil {
		return status.failure("Devin config.json 格式无效："+err.Error(), "请修复 JSON 后重新运行 install.ps1")
	}
	command := devinCommand(root)
	if command == "" {
		return status.notDetected("Devin 未配置 Agent-notify Stop Hook", "请重新运行 install.ps1")
	}
	status.InUse = true
	target := firstCommandPath(command)
	if target == "" || !fileExists(target) {
		return status.failure("Devin Hook 指向的 agent-notify.exe 不存在", "请重新运行 install.ps1")
	}
	for _, name := range []string{"package.json", "extension.js", "acp-bridge.js"} {
		if !fileExists(filepath.Join(paths.DevinExtensionDir, name)) {
			return status.failure("Devin 回复扩展文件不完整", "请重新运行 install.ps1")
		}
	}

	heartbeat := inspectHeartbeat(paths.DevinReplyDir, now)
	switch {
	case heartbeat.fresh:
		return status.connected("Devin 扩展已加载，任务完成通知可推送", "")
	case heartbeat.invalid:
		return status.failure("Devin 扩展心跳无效", "请完全退出并重启 Devin")
	case heartbeat.present:
		return status.pending("扩展已安装，但当前 Devin 进程尚未加载", "请完全退出并重启 Devin")
	default:
		return status.pending("扩展已安装，等待 Devin 加载", "请完全退出并重启 Devin")
	}
}

type heartbeatInfo struct {
	present bool
	fresh   bool
	invalid bool
}

func inspectHeartbeat(dir string, now time.Time) heartbeatInfo {
	info := heartbeatInfo{}
	heartbeatDir := filepath.Join(dir, "heartbeats")
	entries, err := os.ReadDir(heartbeatDir)
	if err == nil {
		for _, entry := range entries {
			if entry.IsDir() || !strings.EqualFold(filepath.Ext(entry.Name()), ".json") {
				continue
			}
			info.merge(readHeartbeat(filepath.Join(heartbeatDir, entry.Name()), now))
		}
	}
	info.merge(readHeartbeat(filepath.Join(dir, "heartbeat.json"), now))
	return info
}

func readHeartbeat(path string, now time.Time) heartbeatInfo {
	data, err := os.ReadFile(path)
	if err != nil {
		if os.IsNotExist(err) {
			return heartbeatInfo{}
		}
		return heartbeatInfo{present: true, invalid: true}
	}
	var payload struct {
		Timestamp time.Time `json:"timestamp"`
	}
	if err := json.Unmarshal(data, &payload); err != nil || payload.Timestamp.IsZero() || payload.Timestamp.After(now.Add(heartbeatFutureSkew)) {
		return heartbeatInfo{present: true, invalid: true}
	}
	info := heartbeatInfo{present: true}
	if now.Sub(payload.Timestamp) <= heartbeatMaxAge {
		info.fresh = true
	}
	return info
}

func (i *heartbeatInfo) merge(other heartbeatInfo) {
	i.present = i.present || other.present
	i.fresh = i.fresh || other.fresh
	i.invalid = i.invalid || other.invalid
}

func newStatus(agentID, name string, enabled bool) Status {
	return Status{Agent: agentID, Name: name, Enabled: enabled}
}

func (s Status) connected(detail, action string) Status {
	s.State = StateConnected
	s.Detail = detail
	s.Action = action
	return s
}

func (s Status) pending(detail, action string) Status {
	s.State = StatePendingRestart
	s.Detail = detail
	s.Action = action
	s.NeedsRestart = true
	return s
}

func (s Status) failure(detail, action string) Status {
	s.State = StateError
	s.Detail = detail
	s.Action = action
	return s
}

func (s Status) failureWithRepair(detail, action, repair string) Status {
	s = s.failure(detail, action)
	s.Repair = repair
	s.Fixable = true
	return s
}

func (s Status) notDetected(detail, action string) Status {
	s.State = StateNotDetected
	s.Detail = detail
	s.Action = action
	return s
}

var (
	notifyLinePattern = regexp.MustCompile(`(?mi)^\s*notify\s*=.*$`)
	doubleQuotePath   = regexp.MustCompile(`"(?:[^"\\]|\\.)*"`)
	singleQuotePath   = regexp.MustCompile(`'(?:[^'\\]|\\.)*'`)
	percentEnvPattern = regexp.MustCompile(`%([^%]+)%`)
)

func notifyLine(content string) string {
	return notifyLinePattern.FindString(content)
}

func firstCommandPath(command string) string {
	for _, pattern := range []*regexp.Regexp{doubleQuotePath, singleQuotePath} {
		match := pattern.FindString(command)
		if match == "" {
			continue
		}
		value, err := strconv.Unquote(match)
		if err != nil {
			// .cmd 中的 Windows 路径不是合法 Go/JSON 字符串，去掉外层引号即可。
			value = strings.Trim(match, `"'`)
		}
		if strings.TrimSpace(value) != "" {
			return expandEnvironment(value)
		}
	}
	fields := strings.Fields(strings.TrimSpace(command))
	for _, field := range fields {
		field = strings.Trim(field, `"`)
		if strings.Contains(strings.ToLower(field), "agent-notify") {
			return expandEnvironment(field)
		}
	}
	return ""
}

func bakedPluginBinary(content string) string {
	const marker = "const BAKED_BIN ="
	for _, line := range strings.Split(content, "\n") {
		line = strings.TrimSpace(line)
		if !strings.HasPrefix(line, marker) {
			continue
		}
		return firstCommandPath(line)
	}
	return ""
}

func defaultAgentNotifyBinary() string {
	home := strings.TrimSpace(os.Getenv("USERPROFILE"))
	if home == "" {
		home, _ = os.UserHomeDir()
	}
	if home == "" {
		return ""
	}
	return filepath.Join(home, "bin", "agent-notify.exe")
}

func expandEnvironment(value string) string {
	value = strings.TrimSpace(value)
	value = percentEnvPattern.ReplaceAllStringFunc(value, func(token string) string {
		name := strings.Trim(token, "%")
		if replacement := os.Getenv(name); replacement != "" {
			return replacement
		}
		return token
	})
	return os.ExpandEnv(value)
}

func resolveRelativePath(value, baseDir string) string {
	if filepath.IsAbs(value) {
		return filepath.Clean(value)
	}
	return filepath.Clean(filepath.Join(baseDir, value))
}

func fileExists(path string) bool {
	if strings.TrimSpace(path) == "" {
		return false
	}
	info, err := os.Stat(path)
	return err == nil && !info.IsDir()
}

func decodeObject(data []byte) (map[string]any, error) {
	var root map[string]any
	if err := json.Unmarshal(data, &root); err != nil {
		return nil, err
	}
	if root == nil {
		return nil, fmt.Errorf("根节点不是对象")
	}
	return root, nil
}

func antigravityCommand(root map[string]any) string {
	group, ok := objectValue(root["agent-notify"])
	if !ok {
		return ""
	}
	return firstCommandInHandlers(group["Stop"])
}

func devinCommand(root map[string]any) string {
	hooks, ok := objectValue(root["hooks"])
	if !ok {
		return ""
	}
	groups, ok := sliceValue(hooks["Stop"])
	if !ok {
		return ""
	}
	for _, rawGroup := range groups {
		group, ok := objectValue(rawGroup)
		if !ok {
			continue
		}
		if command := firstCommandInHandlers(group["hooks"]); command != "" {
			return command
		}
	}
	return ""
}

func firstCommandInHandlers(raw any) string {
	handlers, ok := sliceValue(raw)
	if !ok {
		if handler, ok := objectValue(raw); ok {
			handlers = []any{handler}
		}
	}
	for _, rawHandler := range handlers {
		handler, ok := objectValue(rawHandler)
		if !ok {
			continue
		}
		command, _ := handler["command"].(string)
		if strings.TrimSpace(command) != "" && commandUsesAgentNotify(command) {
			return command
		}
	}
	return ""
}

func commandUsesAgentNotify(command string) bool {
	return strings.Contains(strings.ToLower(command), "agent-notify")
}

func objectValue(raw any) (map[string]any, bool) {
	value, ok := raw.(map[string]any)
	return value, ok
}

func sliceValue(raw any) ([]any, bool) {
	value, ok := raw.([]any)
	return value, ok
}
