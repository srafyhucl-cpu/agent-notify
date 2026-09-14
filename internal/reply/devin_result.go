package reply

import (
	"fmt"
	"strings"
)

// 扩展返回的稳定错误码；detail 只用于补充说明，不参与流程判断。
const (
	devinCodeDesktopUnavailable = "desktop_unavailable"
	devinCodeInvalidJob         = "invalid_job"
	devinCodeSessionNotFound    = "session_not_found"
	devinCodeTurnFailed         = "turn_failed"
)

// 以下错误码只由旧版扩展返回。保留映射，避免旧扩展与新程序混装时提示退化成原始报错。
const (
	devinCodeAgentMissing       = "agent_missing"
	devinCodeNotAuthenticated   = "not_authenticated"
	devinCodeSessionLocked      = "session_locked"
	devinCodeWorkspaceUntrusted = "workspace_untrusted"
)

// devinResultError 把桌面扩展返回的稳定错误码映射成微信可读提示，
// 不回显可能包含回复内容的原始错误。
func devinResultError(code, detail string) error {
	switch strings.ToLower(strings.TrimSpace(code)) {
	case devinCodeDesktopUnavailable:
		return fmt.Errorf("devin reply: 当前 Devin 桌面端未提供精确回复能力，请更新 Devin 桌面端后重启")
	case devinCodeInvalidJob:
		return fmt.Errorf("devin reply: 回复任务无效，请重新引用原通知后再试")
	case devinCodeSessionNotFound:
		return fmt.Errorf("devin reply: 目标 Devin 会话不存在或已删除，请确认会话后再试")
	case devinCodeAgentMissing:
		return fmt.Errorf("devin reply: 未找到 Devin 桌面端自带的 Agent，请更新或重启 Devin 后重试")
	case devinCodeNotAuthenticated:
		return fmt.Errorf("devin reply: Devin 桌面端登录状态已失效，请在 Devin 中重新登录后重试")
	case devinCodeSessionLocked:
		return fmt.Errorf("devin reply: 目标会话正被 Devin 占用，请等本轮结束后再回复；若已无运行任务，请在 Devin 中关闭该会话或重启 Devin 后重试")
	case devinCodeWorkspaceUntrusted:
		return fmt.Errorf("devin reply: 目标工作区在 Devin 中未受信任，请先在 Devin 桌面端信任该工作区")
	case devinCodeTurnFailed:
		if detail = strings.TrimSpace(detail); detail != "" {
			return fmt.Errorf("devin reply: 回复未执行成功：%s", detail)
		}
		return fmt.Errorf("devin reply: 回复未执行成功，请查看 Devin 窗口中的错误提示")
	}

	// 旧版本扩展只回传错误文本，这里保留按特征识别的兼容分支。
	normalized := strings.ToLower(strings.TrimSpace(detail))
	switch {
	case strings.Contains(normalized, "session not found"),
		strings.Contains(normalized, "cascade not found"),
		strings.Contains(normalized, "unknown cascade"):
		return fmt.Errorf("devin reply: 目标 Devin 会话不存在或已删除，请确认会话后再试")
	case strings.Contains(normalized, "invalid_message"),
		strings.Contains(normalized, "invalid_session_id"):
		return fmt.Errorf("devin reply: 回复任务无效，请重新引用原通知后再试")
	case strings.Contains(normalized, "command not found"),
		strings.Contains(normalized, "not available"):
		return fmt.Errorf("devin reply: 当前 Devin 桌面端未提供精确回复能力，请更新 Devin 桌面端后重启")
	case strings.TrimSpace(detail) != "":
		return fmt.Errorf("devin reply: %s", strings.TrimSpace(detail))
	}
	return fmt.Errorf("devin reply: 回复未执行成功，请查看 Devin 窗口中的错误提示")
}
