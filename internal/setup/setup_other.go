//go:build !windows

package setup

import (
	"context"
	"errors"
)

func runHidden(context.Context, string, ...string) ([]byte, error) {
	return nil, errors.New("首次接入仅支持 Windows")
}
