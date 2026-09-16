//go:build !windows

package update

import (
	"context"
	"errors"
)

func inspectSignature(context.Context, string) (SignatureInfo, error) {
	return SignatureInfo{}, errors.New("签名校验仅在 Windows 上可用")
}
