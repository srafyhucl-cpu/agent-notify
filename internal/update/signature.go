package update

import (
	"context"
	"errors"
	"fmt"
	"os"
	"strings"
)

// SignatureInfo 描述一次 Authenticode 检查结果。
type SignatureInfo struct {
	Status     string
	Thumbprint string
}

const (
	signatureStatusValid        = "Valid"
	signatureStatusNotSigned    = "NotSigned"
	signatureStatusHashMismatch = "HashMismatch"
	signatureStatusNotTrusted   = "NotTrusted"

	// defaultSignatureThumbprint 为空表示"不默认限定签名者"。启用代码签名后把证书指纹
	// 填在这里，所有客户端都会只信任该签名者（详见 docs/code-signing.md）。
	defaultSignatureThumbprint = ""
)

// verifyArtifactSignature 校验更新产物（最终要执行的程序）的 Authenticode 签名。
// 默认策略：签名无效一律拒绝；未签名默认放行（保持向后兼容）。
// AGENT_NOTIFY_REQUIRE_SIGNATURE=1 要求必须签名；AGENT_NOTIFY_SIGNATURE_THUMBPRINT
// 配置后只信任指定指纹（逗号/分号/空格分隔）。
func verifyArtifactSignature(ctx context.Context, path string) error {
	info, err := inspectSignature(ctx, path)
	if err != nil {
		return err
	}
	return signaturePolicy(info, signatureRequiredFromEnv(), signatureThumbprintsFromEnv())
}

// signaturePolicy 是签名放行规则的纯函数，便于测试。
func signaturePolicy(info SignatureInfo, required bool, thumbprints []string) error {
	status := strings.TrimSpace(info.Status)
	pinned := normalizeThumbprints(thumbprints)
	if len(pinned) > 0 {
		if status != signatureStatusValid {
			return fmt.Errorf("更新包签名校验失败：已配置信任指纹，但安装器状态为 %s，拒绝安装", displaySignatureStatus(status))
		}
		actual := normalizeThumbprint(info.Thumbprint)
		if actual == "" || !containsThumbprint(pinned, actual) {
			return fmt.Errorf("更新包签名者不匹配：实际 %s，不在信任列表中，拒绝安装", displayThumbprint(info.Thumbprint))
		}
		return nil
	}

	switch status {
	case signatureStatusValid:
		return nil
	case signatureStatusHashMismatch, signatureStatusNotTrusted:
		// 有签名但校验失败：最明确的篡改/不可信信号，任何策略下都拒绝。
		return fmt.Errorf("更新包签名无效（%s），拒绝安装", displaySignatureStatus(status))
	default:
		// NotSigned / UnknownError / NotSupportedFileFormat / Incompatible：没有可用签名或无法识别。
		if required {
			return errors.New("更新包未签名，已按 AGENT_NOTIFY_REQUIRE_SIGNATURE 配置拒绝安装")
		}
		return nil
	}
}

func signatureRequiredFromEnv() bool {
	raw := strings.ToLower(strings.TrimSpace(os.Getenv("AGENT_NOTIFY_REQUIRE_SIGNATURE")))
	return raw == "1" || raw == "true" || raw == "on"
}

func signatureThumbprintsFromEnv() []string {
	if raw := strings.TrimSpace(os.Getenv("AGENT_NOTIFY_SIGNATURE_THUMBPRINT")); raw != "" {
		return strings.Split(strings.NewReplacer(";", ",").Replace(raw), ",")
	}
	if pinned := strings.TrimSpace(defaultSignatureThumbprint); pinned != "" {
		return []string{pinned}
	}
	return nil
}

func normalizeThumbprints(values []string) []string {
	normalized := make([]string, 0, len(values))
	for _, value := range values {
		if thumbprint := normalizeThumbprint(value); thumbprint != "" {
			normalized = append(normalized, thumbprint)
		}
	}
	return normalized
}

func normalizeThumbprint(value string) string {
	compacted := strings.Join(strings.Fields(strings.TrimSpace(value)), "")
	return strings.ToUpper(strings.ReplaceAll(compacted, ":", ""))
}

func containsThumbprint(list []string, value string) bool {
	for _, item := range list {
		if item == value {
			return true
		}
	}
	return false
}

func displaySignatureStatus(status string) string {
	if strings.TrimSpace(status) == "" {
		return "未知"
	}
	return status
}

func displayThumbprint(value string) string {
	normalized := normalizeThumbprint(value)
	if normalized == "" {
		return "无签名者"
	}
	return normalized
}
