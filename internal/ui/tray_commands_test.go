//go:build windows

package ui

import (
	"testing"

	"github.com/srafyhucl-cpu/agent-notify/internal/agentmeta"
)

// 托盘菜单命令与 Agent 的映射必须双向一致。
func TestTrayAgentCommandsRoundTrip(t *testing.T) {
	seen := make(map[uintptr]string, len(trayAgentCommands))
	for _, command := range trayAgentCommands {
		if previous, exists := seen[command.ID]; exists {
			t.Fatalf("命令 ID %d 被 %s 与 %s 重复使用", command.ID, previous, command.AgentID)
		}
		seen[command.ID] = command.AgentID

		agentID, ok := trayAgentIDForCommand(int(command.ID))
		if !ok || agentID != command.AgentID {
			t.Fatalf("trayAgentIDForCommand(%d) = (%q, %v), want (%q, true)", command.ID, agentID, ok, command.AgentID)
		}
		commandID, ok := trayCommandForAgent(command.AgentID)
		if !ok || commandID != command.ID {
			t.Fatalf("trayCommandForAgent(%q) = (%d, %v), want (%d, true)", command.AgentID, commandID, ok, command.ID)
		}
	}

	if _, ok := trayAgentIDForCommand(9999); ok {
		t.Fatal("未知命令 ID 不应映射到 Agent")
	}
	if _, ok := trayCommandForAgent("unknown-agent"); ok {
		t.Fatal("未知 Agent 不应映射到命令")
	}
}

// Agent 命令不能与固定菜单命令撞号，否则托盘操作会触发错误动作。
func TestTrayCommandsDoNotCollideWithMenuCommands(t *testing.T) {
	fixed := map[uintptr]string{
		IDM_TOGGLE_SHOW: "显示/隐藏",
		IDM_HISTORY:     "推送历史",
		IDM_SETTINGS:    "设置",
		IDM_TEST_PUSH:   "测试推送",
		IDM_EXIT:        "退出",
		IDM_UPDATE:      "检查更新",
	}
	for _, command := range trayAgentCommands {
		if name, exists := fixed[command.ID]; exists {
			t.Fatalf("Agent 命令 %s 与固定命令 %s 撞号（ID %d）", command.AgentID, name, command.ID)
		}
	}
}

// 每个受支持的 Agent 都应该能在托盘菜单里开关。
func TestTrayCoversAllAgents(t *testing.T) {
	for _, descriptor := range agentmeta.All() {
		if _, ok := trayCommandForAgent(descriptor.ID); !ok {
			t.Fatalf("Agent %s 缺少托盘开关命令", descriptor.ID)
		}
	}
}
