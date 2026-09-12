package clawbot

import (
	"fmt"
	"strings"

	qrcode "github.com/skip2/go-qrcode"
)

// RenderQR renders QR content with Unicode half blocks for terminals that use a
// dark background. The returned string includes the QR quiet zone.
func RenderQR(content string) (string, error) {
	content = strings.TrimSpace(content)
	if content == "" {
		return "", fmt.Errorf("clawbot: QR content is empty")
	}
	code, err := qrcode.New(content, qrcode.Medium)
	if err != nil {
		return "", fmt.Errorf("clawbot: render QR: %w", err)
	}
	return code.ToSmallString(false), nil
}
