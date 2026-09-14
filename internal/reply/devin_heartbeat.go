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
	devinHeartbeatMaxAge     = 30 * time.Second
	devinHeartbeatFutureSkew = 5 * time.Second
)

type devinHeartbeat struct {
	Ready     bool      `json:"ready"`
	Timestamp time.Time `json:"timestamp"`
}

// requireDevinHeartbeat 确认至少一个 Devin 扩展实例在线，并且本机 Devin 桌面端
// 提供精确会话回复命令（由扩展探测后写入 heartbeat）。
func requireDevinHeartbeat(dir string, now time.Time) error {
	heartbeatDir := filepath.Join(dir, "heartbeats")
	entries, err := os.ReadDir(heartbeatDir)
	if err != nil {
		if os.IsNotExist(err) {
			return fmt.Errorf("Devin 引用回复扩展未运行")
		}
		return fmt.Errorf("devin reply: read heartbeat directory: %w", err)
	}

	sawCapable := false
	sawUnsupported := false
	sawMalformed := false
	sawInvalidTime := false
	for _, entry := range entries {
		if entry.IsDir() || !strings.EqualFold(filepath.Ext(entry.Name()), ".json") {
			continue
		}
		data, err := os.ReadFile(filepath.Join(heartbeatDir, entry.Name()))
		if err != nil {
			sawMalformed = true
			continue
		}
		var heartbeat devinHeartbeat
		if err := json.Unmarshal(data, &heartbeat); err != nil {
			sawMalformed = true
			continue
		}
		capable, fresh, valid := classifyDevinHeartbeat(heartbeat, now)
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
		return fmt.Errorf("当前 Devin 桌面端未提供精确回复能力，请更新 Devin 桌面端后重启")
	case sawCapable:
		return fmt.Errorf("Devin 引用回复扩展已离线，请重新打开 Devin 桌面端")
	case sawInvalidTime:
		return fmt.Errorf("Devin 引用回复扩展心跳时间无效")
	case sawMalformed:
		return fmt.Errorf("Devin 引用回复扩展状态无效")
	default:
		return fmt.Errorf("Devin 引用回复扩展未运行")
	}
}

func classifyDevinHeartbeat(heartbeat devinHeartbeat, now time.Time) (capable, fresh, valid bool) {
	if heartbeat.Timestamp.IsZero() || heartbeat.Timestamp.After(now.Add(devinHeartbeatFutureSkew)) {
		return heartbeat.Ready, false, false
	}
	return heartbeat.Ready, now.Sub(heartbeat.Timestamp) <= devinHeartbeatMaxAge, true
}
