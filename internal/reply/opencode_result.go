package reply

import (
	"errors"
	"fmt"
	"strings"
)

var errOpenCodeResultUnconfirmed = errors.New("opencode reply: 未在有效期内确认会话 prompt 执行结果")

// OpenCodeFailureReporter reports a durable OpenCode job that failed after the
// synchronous Queue result window. Implementations must not retry the job.
type OpenCodeFailureReporter func(sessionID, text string, err error)

func consumeOpenCodeResult(path string) (bool, error) {
	return consumeSpoolResult(path, "opencode reply", openCodeResultError)
}

// openCodeResultError 保持 OpenCode 现有的文本识别逻辑；code 由 OpenCode
// 扩展自行描述，暂不参与映射。
func openCodeResultError(_ string, detail string) error {
	normalized := strings.ToLower(strings.TrimSpace(detail))
	if strings.Contains(normalized, "session.notfounderror") ||
		strings.Contains(normalized, "session not found") {
		return fmt.Errorf("opencode reply: 目标 OpenCode 会话不存在或已删除，请确认会话后再试")
	}
	return fmt.Errorf("opencode reply: %s", detail)
}
