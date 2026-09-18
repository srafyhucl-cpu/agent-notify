package reply

import (
	"encoding/json"
	"fmt"
	"os"
	"path/filepath"
	"strings"
	"time"
)

const (
	commandCodeHeartbeatMaxAge     = 30 * time.Second
	commandCodeHeartbeatFutureSkew = 5 * time.Second
)

type commandCodeHeartbeat struct {
	Ready     bool      `json:"ready"`
	Timestamp time.Time `json:"timestamp"`
	SessionID string    `json:"sessionId"`
}

// requireCommandCodeHeartbeat 要求存在一个新鲜、具备注入能力、且 sessionId 与目标会话
// 一致的心跳。Command Code 的 queueMessage 只能注入自身会话，多开时若不做会话匹配就会
// 投错窗口，因此这里按会话精确定位，匹配不到即明确失败，不回退到别的会话。
func requireCommandCodeHeartbeat(dir, sessionID string, now time.Time) error {
	target := strings.TrimSpace(sessionID)
	if target == "" {
		return fmt.Errorf("Command Code 引用回复缺少目标会话")
	}
	heartbeatDir := filepath.Join(dir, commandCodeHeartbeatDirName)
	entries, err := os.ReadDir(heartbeatDir)
	if err != nil {
		if os.IsNotExist(err) {
			return fmt.Errorf("Command Code mod 未运行")
		}
		return fmt.Errorf("commandcode reply: read heartbeat directory: %w", err)
	}

	sawOtherSession := false
	sawStale := false
	sawUnsupported := false
	sawInvalid := false
	for _, entry := range entries {
		if entry.IsDir() || !strings.EqualFold(filepath.Ext(entry.Name()), ".json") {
			continue
		}
		data, err := os.ReadFile(filepath.Join(heartbeatDir, entry.Name()))
		if err != nil {
			continue
		}
		var heartbeat commandCodeHeartbeat
		if err := json.Unmarshal(data, &heartbeat); err != nil {
			continue
		}
		if strings.TrimSpace(heartbeat.SessionID) != target {
			sawOtherSession = true
			continue
		}
		capable, fresh, valid := classifyHeartbeat(
			heartbeatState{Ready: heartbeat.Ready, Timestamp: heartbeat.Timestamp},
			now,
			commandCodeHeartbeatMaxAge,
			commandCodeHeartbeatFutureSkew,
		)
		if !valid {
			sawInvalid = true
			continue
		}
		if !fresh {
			sawStale = true
			continue
		}
		if !capable {
			sawUnsupported = true
			continue
		}
		return nil
	}
	// 必须扫完整个目录再判定：进程重启会留下僵死心跳，若在第一条匹配但过期的
	// 文件上提前返回，就会把仍然存活的实例误判成"未在运行"。
	switch {
	case sawUnsupported:
		return fmt.Errorf("当前 Command Code 不支持会话注入（queueMessage）")
	case sawStale:
		return fmt.Errorf("Command Code 目标会话未在运行，请先打开该会话")
	case sawInvalid:
		return fmt.Errorf("Command Code mod 心跳时间无效")
	case sawOtherSession:
		return fmt.Errorf("Command Code 目标会话未在运行，请先打开该会话")
	default:
		return fmt.Errorf("Command Code mod 未运行")
	}
}
