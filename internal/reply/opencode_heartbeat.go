package reply

import (
	"encoding/json"
	"fmt"
	"os"
	"path/filepath"
	"strings"
	"time"
)

type openCodeHeartbeat struct {
	Ready     bool      `json:"ready"`
	Timestamp time.Time `json:"timestamp"`
}

func requireOpenCodeHeartbeat(dir string, now time.Time) error {
	heartbeatDir := filepath.Join(dir, openCodeHeartbeatDirName)
	entries, err := os.ReadDir(heartbeatDir)
	if err == nil {
		directoryErr := checkOpenCodeHeartbeatDir(heartbeatDir, entries, now)
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
func checkOpenCodeHeartbeatDir(dir string, entries []os.DirEntry, now time.Time) error {
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
		var heartbeat openCodeHeartbeat
		if err := json.Unmarshal(data, &heartbeat); err != nil {
			sawMalformed = true
			continue
		}
		capable, fresh, valid := classifyOpenCodeHeartbeat(heartbeat, now)
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
	if sawUnsupported {
		return fmt.Errorf("当前 OpenCode 插件不支持会话 prompt（session.prompt / promptAsync）")
	}
	if sawCapable {
		return fmt.Errorf("OpenCode 引用回复插件已离线")
	}
	if sawInvalidTime {
		return fmt.Errorf("OpenCode 引用回复插件心跳时间无效")
	}
	if sawMalformed {
		return fmt.Errorf("OpenCode 引用回复插件状态无效")
	}
	return fmt.Errorf("OpenCode 引用回复插件未运行")
}

func checkOpenCodeHeartbeat(data []byte, now time.Time) error {
	var heartbeat openCodeHeartbeat
	if err := json.Unmarshal(data, &heartbeat); err != nil {
		return fmt.Errorf("OpenCode 引用回复插件状态无效")
	}
	capable, fresh, valid := classifyOpenCodeHeartbeat(heartbeat, now)
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

func classifyOpenCodeHeartbeat(heartbeat openCodeHeartbeat, now time.Time) (capable, fresh, valid bool) {
	if heartbeat.Timestamp.IsZero() || heartbeat.Timestamp.After(now.Add(openCodeHeartbeatFutureSkew)) {
		return heartbeat.Ready, false, false
	}
	return heartbeat.Ready, now.Sub(heartbeat.Timestamp) <= openCodeHeartbeatMaxAge, true
}
