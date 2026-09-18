package reply

import (
	"errors"
	"fmt"
	"strings"
)

var errCommandCodeResultUnconfirmed = errors.New("commandcode reply: 未在有效期内确认会话注入结果")

func consumeCommandCodeResult(path string) (bool, error) {
	return consumeSpoolResult(path, "commandcode reply", commandCodeResultError)
}

// commandCodeResultError 把 Command Code mod 报回的稳定错误码映射成微信可读提示。
func commandCodeResultError(code, detail string) error {
	normalized := strings.ToLower(strings.TrimSpace(code))
	text := strings.TrimSpace(detail)
	switch normalized {
	case "session_not_running":
		return fmt.Errorf("Command Code 目标会话未在运行，请先打开该会话后重试")
	case "session_idle":
		return fmt.Errorf("Command Code 目标会话当前空闲，mod 无法主动唤醒会话；请先在 Command Code 里发送任意消息后再引用回复")
	case "window_closed":
		if text != "" {
			return fmt.Errorf("Command Code %s；请等下一次通知发出后再引用回复", text)
		}
		return fmt.Errorf("Command Code 回复窗口已过，请等下一次通知发出后再引用回复")
	case "invalid_job":
		return fmt.Errorf("Command Code 引用回复任务无效")
	case "inject_failed":
		if text != "" {
			return fmt.Errorf("Command Code 注入失败：%s", text)
		}
		return fmt.Errorf("Command Code 注入失败")
	}
	if text != "" {
		return fmt.Errorf("commandcode reply: %s", text)
	}
	return fmt.Errorf("commandcode reply: 会话注入失败")
}
