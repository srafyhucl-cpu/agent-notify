package reply

import (
	"errors"
	"os"
	"path/filepath"
	"strings"
	"time"
)

// heartbeatState 是单个心跳文件解析后的公共状态；各 Agent 的 JSON 结构由
// 各自的 parse 闭包转换成该结构。
type heartbeatState struct {
	Ready     bool
	Timestamp time.Time
}

// heartbeatMessages 汇总某一 Agent 的心跳门禁文案；不同 Agent 仅文案不同。
type heartbeatMessages struct {
	Unsupported string // 桌面端/插件不支持精确回复
	Offline     string
	InvalidTime string
	Malformed   string
	NotRunning  string
}

// classifyHeartbeat 判断心跳是否具备精确回复能力、是否新鲜、时间是否有效。
func classifyHeartbeat(state heartbeatState, now time.Time, maxAge, futureSkew time.Duration) (capable, fresh, valid bool) {
	if state.Timestamp.IsZero() || state.Timestamp.After(now.Add(futureSkew)) {
		return state.Ready, false, false
	}
	return state.Ready, now.Sub(state.Timestamp) <= maxAge, true
}

// checkHeartbeatEntries 遍历心跳目录中的 .json 文件，存在新鲜且 capable 的实例时
// 返回 nil；否则按 Unsupported > Offline（过期但 capable）> InvalidTime > Malformed >
// NotRunning 的优先级返回对应错误。该优先级与各 Agent 原有实现保持一致。
func checkHeartbeatEntries(dir string, entries []os.DirEntry, parse func([]byte) (heartbeatState, bool), now time.Time, maxAge, futureSkew time.Duration, messages heartbeatMessages) error {
	sawCapable := false
	sawUnsupported := false
	sawMalformed := false
	sawInvalidTime := false
	for _, entry := range entries {
		if entry.IsDir() || !strings.EqualFold(filepath.Ext(entry.Name()), ".json") {
			continue
		}
		data, err := os.ReadFile(filepath.Join(dir, entry.Name()))
		if err != nil {
			sawMalformed = true
			continue
		}
		state, ok := parse(data)
		if !ok {
			sawMalformed = true
			continue
		}
		capable, fresh, valid := classifyHeartbeat(state, now, maxAge, futureSkew)
		if !valid {
			sawInvalidTime = true
			continue
		}
		if !fresh {
			if capable {
				sawCapable = true
			}
			continue
		}
		if !capable {
			sawUnsupported = true
			continue
		}
		return nil
	}

	switch {
	case sawUnsupported:
		return errors.New(messages.Unsupported)
	case sawCapable:
		return errors.New(messages.Offline)
	case sawInvalidTime:
		return errors.New(messages.InvalidTime)
	case sawMalformed:
		return errors.New(messages.Malformed)
	default:
		return errors.New(messages.NotRunning)
	}
}
