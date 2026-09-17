package clawbot

import (
	"fmt"
	"io"
)

// maxResponseBytes 限制单次 HTTP 响应体的最大读取量，避免被超大响应打爆内存。
const maxResponseBytes = 8 << 20 // 8 MiB

// readResponseBody 读取响应体，超过上限直接报错而不是静默截断。
func readResponseBody(r io.Reader) ([]byte, error) {
	data, err := io.ReadAll(io.LimitReader(r, maxResponseBytes+1))
	if err != nil {
		return nil, err
	}
	if len(data) > maxResponseBytes {
		return nil, fmt.Errorf("clawbot: response body exceeds %d bytes", maxResponseBytes)
	}
	return data, nil
}
