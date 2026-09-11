package marker

import (
	"fmt"
	"os"
	"path/filepath"
	"strings"
	"time"
)

// IsOff returns true if marker file exists (meaning notification is OFF).
func IsOff(path string) bool {
	if path == "" {
		return false
	}
	_, err := os.Stat(path)
	return err == nil
}

// SetMarker adjusts the marker file based on mode: "On", "Off", or "Flip".
// Returns the resulting status: "ON" or "OFF".
func SetMarker(path string, mode string) (string, error) {
	if path == "" {
		return "ON", fmt.Errorf("marker path is empty")
	}

	isCurrentlyOff := IsOff(path)

	modeLower := strings.ToLower(strings.TrimSpace(mode))
	turnOn := false
	switch modeLower {
	case "on":
		turnOn = true
	case "off":
		turnOn = false
	default: // flip
		turnOn = isCurrentlyOff
	}

	if turnOn {
		if isCurrentlyOff {
			_ = os.Remove(path)
		}
		return "ON", nil
	}

	// Turn Off: ensure directory exists and write timestamp
	dir := filepath.Dir(path)
	if err := os.MkdirAll(dir, 0755); err != nil {
		return "ON", err
	}

	content := fmt.Sprintf("off %s\n", time.Now().Format(time.RFC3339))
	if err := os.WriteFile(path, []byte(content), 0644); err != nil {
		return "ON", err
	}

	return "OFF", nil
}
