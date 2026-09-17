package reply

import (
	"encoding/json"
	"fmt"
	"os"
	"path/filepath"
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

var devinHeartbeatMessages = heartbeatMessages{
	Unsupported: "当前 Devin 桌面端未提供精确回复能力，请更新 Devin 桌面端后重启",
	Offline:     "Devin 引用回复扩展已离线，请重新打开 Devin 桌面端",
	InvalidTime: "Devin 引用回复扩展心跳时间无效",
	Malformed:   "Devin 引用回复扩展状态无效",
	NotRunning:  "Devin 引用回复扩展未运行",
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

	return checkHeartbeatEntries(heartbeatDir, entries, parseDevinHeartbeat, now, devinHeartbeatMaxAge, devinHeartbeatFutureSkew, devinHeartbeatMessages)
}

// parseDevinHeartbeat 把 Devin 心跳 JSON 解析成公共状态；解析失败由调用方按
// 「状态无效」处理。
func parseDevinHeartbeat(data []byte) (heartbeatState, bool) {
	var heartbeat devinHeartbeat
	if err := json.Unmarshal(data, &heartbeat); err != nil {
		return heartbeatState{}, false
	}
	return heartbeatState{Ready: heartbeat.Ready, Timestamp: heartbeat.Timestamp}, true
}
