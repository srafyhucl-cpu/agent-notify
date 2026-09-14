package agent

import (
	"bytes"
	"encoding/json"
	"errors"
	"fmt"
	"io"
)

const maxHookInputBytes = 1 << 20

// ErrEmptyHookInput means an agent invoked a hook without a JSON payload.
var ErrEmptyHookInput = errors.New("hook input is empty")

func readHookJSON(reader io.Reader, target any) error {
	if reader == nil {
		return errors.New("hook input is unavailable")
	}
	data, err := io.ReadAll(io.LimitReader(reader, maxHookInputBytes+1))
	if err != nil {
		return fmt.Errorf("read hook input: %w", err)
	}
	if len(data) > maxHookInputBytes {
		return fmt.Errorf("hook input exceeds %d bytes", maxHookInputBytes)
	}
	data = bytes.TrimSpace(bytes.TrimPrefix(data, []byte{0xef, 0xbb, 0xbf}))
	if len(data) == 0 {
		return ErrEmptyHookInput
	}
	if err := json.Unmarshal(data, target); err != nil {
		return fmt.Errorf("decode hook input: %w", err)
	}
	return nil
}
