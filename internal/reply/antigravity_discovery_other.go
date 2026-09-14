//go:build !windows

package reply

import (
	"context"
	"errors"
)

// newAntigravityResolver 在非 Windows 平台不可用：Antigravity 桌面端与
// language_server.exe 只在 Windows 上安装。
func newAntigravityResolver(_ string) EndpointResolver {
	return func(context.Context, string) (AntigravityEndpoint, error) {
		return AntigravityEndpoint{}, errors.New(
			"Antigravity 桌面端引用回复仅支持 Windows",
		)
	}
}

func resolveAntigravityLanguageServerBinary() (string, error) {
	return "", errors.New("Antigravity 桌面端引用回复仅支持 Windows")
}
