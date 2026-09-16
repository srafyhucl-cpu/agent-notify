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
	signatureStatusUnknownError = "UnknownError"

	// defaultSignatureThumbprint 是内置的信任指纹：客户端只接受由该证书签名的更新包
	// （自签名的签名状态是 UnknownError/NotTrusted，指纹匹配即放行；篡改仍会被拒绝）。
	// 轮换证书时必须先更新这里并发版，详见 docs/code-signing.md。
	defaultSignatureThumbprint = "EDF9E283DF2407B318E65D59BB430FD546509ACD"
)

// verifyArtifactSignature 校验更新产物（最终要执行的程序）的 Authenticode 签名。
// 默认策略：签名无效一律拒绝；未签名默认放行（保持向后兼容）。
// AGENT_NOTIFY_REQUIRE_SIGNATURE=1 要求必须签名；AGENT_NOTIFY_SIGNATURE_THUMBPRINT
// 配置后只信任指定指纹（逗号/分号/空格分隔）。
// resolveSignatureThumbprints 默认为内置指纹 + 环境变量覆盖；测试可临时替换，
// 以便用未签名的桩产物验证 Prepare 的其它行为。
var resolveSignatureThumbprints = signatureThumbprintsFromEnv

func verifyArtifactSignature(ctx context.Context, path string) error {
	info, err := inspectSignature(ctx, path)
	if err != nil {
		return err
	}
	return signaturePolicy(info, signatureRequiredFromEnv(), resolveSignatureThumbprints())
}

// signaturePolicy 是签名放行规则的纯函数，便于测试。
func signaturePolicy(info SignatureInfo, required bool, thumbprints []string) error {
	status := strings.TrimSpace(info.Status)
	pinned := normalizeThumbprints(thumbprints)
	if len(pinned) > 0 {
		// 配置了指纹后，指纹本身就是信任锚：自签名/根不受信（UnknownError、NotTrusted）
		// 只要指纹匹配就放行，但篡改类状态（HashMismatch 等）一律拒绝。
		if !acceptableWhenPinned(status) {
			return fmt.Errorf("更新包签名状态异常（%s），拒绝安装", displaySignatureStatus(status))
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

// acceptableWhenPinned 判断"签名存在且未被判为篡改"的状态：自签名证书因根不受信
// 会得到 UnknownError/NotTrusted，此时以指纹作为信任锚；HashMismatch 等篡改信号必须拒绝。
func acceptableWhenPinned(status string) bool {
	switch status {
	case signatureStatusValid, signatureStatusUnknownError, signatureStatusNotTrusted:
		return true
	default:
		return false
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
