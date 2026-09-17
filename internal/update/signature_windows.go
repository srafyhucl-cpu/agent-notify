//go:build windows

package update

import (
	"context"
	"encoding/json"
	"fmt"
	"os/exec"
	"strings"

	"github.com/srafyhucl-cpu/agent-notify/internal/sysproc"
)

// inspectSignature 用系统自带的 Authenticode 检查读取签名状态与签名者指纹。
func inspectSignature(ctx context.Context, path string) (SignatureInfo, error) {
	quoted := strings.ReplaceAll(strings.TrimSpace(path), "'", "''")
	script := "$ErrorActionPreference='Stop'; " +
		"$sig = Get-AuthenticodeSignature -LiteralPath '" + quoted + "'; " +
		"$thumb = ''; if ($sig.SignerCertificate) { $thumb = [string]$sig.SignerCertificate.Thumbprint }; " +
		"[pscustomobject]@{ Status = [string]$sig.Status; Thumbprint = $thumb } | ConvertTo-Json -Compress"
	command := exec.CommandContext(ctx, "powershell.exe", "-NoProfile", "-ExecutionPolicy", "Bypass", "-Command", script)
	sysproc.ConfigureHidden(command)
	output, err := command.Output()
	if err != nil {
		return SignatureInfo{}, fmt.Errorf("读取安装包签名失败：%w", err)
	}

	var parsed struct {
		Status     string `json:"Status"`
		Thumbprint string `json:"Thumbprint"`
	}
	if err := json.Unmarshal(output, &parsed); err != nil {
		return SignatureInfo{}, fmt.Errorf("解析签名校验结果失败：%w", err)
	}
	return SignatureInfo{
		Status:     strings.TrimSpace(parsed.Status),
		Thumbprint: strings.TrimSpace(parsed.Thumbprint),
	}, nil
}
