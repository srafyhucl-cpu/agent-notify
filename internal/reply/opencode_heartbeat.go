package reply

import (
	"encoding/json"
	"fmt"
	"os"
	"path/filepath"
	"time"
)

type openCodeHeartbeat struct {
	Ready     bool      `json:"ready"`
	Timestamp time.Time `json:"timestamp"`
}

var openCodeHeartbeatMessages = heartbeatMessages{
	Unsupported: "当前 OpenCode 插件不支持会话 prompt（session.prompt / promptAsync）",
	Offline:     "OpenCode 引用回复插件已离线",
	InvalidTime: "OpenCode 引用回复插件心跳时间无效",
	Malformed:   "OpenCode 引用回复插件状态无效",
	NotRunning:  "OpenCode 引用回复插件未运行",
}

func requireOpenCodeHeartbeat(dir string, now time.Time) error {
	heartbeatDir := filepath.Join(dir, openCodeHeartbeatDirName)
	entries, err := os.ReadDir(heartbeatDir)
	if err == nil {
		directoryErr := checkHeartbeatEntries(heartbeatDir, entries, parseOpenCodeHeartbeat, now, openCodeHeartbeatMaxAge, openCodeHeartbeatFutureSkew, openCodeHeartbeatMessages)
		if directoryErr == nil {
			return nil
		}
		// Keep accepting a fresh legacy heartbeat during a rolling plugin
		// upgrade even if an earlier instance created the lease directory.
		if legacyErr := checkOpenCodeLegacyHeartbeat(dir, now); legacyErr == nil {
			return nil
		}
		return directoryErr
	}
	if !os.IsNotExist(err) {
		return fmt.Errorf("opencode reply: read heartbeat directory: %w", err)
	}

	return checkOpenCodeLegacyHeartbeat(dir, now)
}

func checkOpenCodeLegacyHeartbeat(dir string, now time.Time) error {
	legacyPath := filepath.Join(dir, openCodeLegacyHeartbeatFile)
	data, err := os.ReadFile(legacyPath)
	if err != nil {
		if os.IsNotExist(err) {
			return fmt.Errorf("OpenCode 引用回复插件未运行")
		}
		return fmt.Errorf("opencode reply: read heartbeat: %w", err)
	}
	return checkOpenCodeHeartbeat(data, now)
}

func checkOpenCodeHeartbeat(data []byte, now time.Time) error {
	var heartbeat openCodeHeartbeat
	if err := json.Unmarshal(data, &heartbeat); err != nil {
		return fmt.Errorf("OpenCode 引用回复插件状态无效")
	}
	capable, fresh, valid := classifyHeartbeat(heartbeatState{Ready: heartbeat.Ready, Timestamp: heartbeat.Timestamp}, now, openCodeHeartbeatMaxAge, openCodeHeartbeatFutureSkew)
	if !valid {
		return fmt.Errorf("OpenCode 引用回复插件心跳时间无效")
	}
	if !fresh {
		return fmt.Errorf("OpenCode 引用回复插件已离线")
	}
	if !capable {
		return fmt.Errorf("当前 OpenCode 插件不支持会话 prompt（session.prompt / promptAsync）")
	}
	return nil
}

// parseOpenCodeHeartbeat 把 OpenCode 心跳 JSON 解析成公共状态；解析失败由调用方
// 按「状态无效」处理。
func parseOpenCodeHeartbeat(data []byte) (heartbeatState, bool) {
	var heartbeat openCodeHeartbeat
	if err := json.Unmarshal(data, &heartbeat); err != nil {
		return heartbeatState{}, false
	}
	return heartbeatState{Ready: heartbeat.Ready, Timestamp: heartbeat.Timestamp}, true
}
