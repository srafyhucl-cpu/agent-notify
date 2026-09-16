package update

import (
	"context"
	"os"
	"path/filepath"
	"runtime"
	"testing"
)

// clearSignaturePin 让使用未签名桩产物的 Prepare 测试不受内置信任指纹影响。
func clearSignaturePin(t *testing.T) {
	t.Helper()
	original := resolveSignatureThumbprints
	resolveSignatureThumbprints = func() []string { return nil }
	t.Cleanup(func() { resolveSignatureThumbprints = original })
}

func TestSignaturePolicy(t *testing.T) {
	tests := []struct {
		name       string
		status     string
		thumbprint string
		required   bool
		pinned     []string
		wantErr    bool
	}{
		{name: "未签名默认放行", status: "NotSigned"},
		{name: "要求签名时拒绝未签名", status: "NotSigned", required: true, wantErr: true},
		{name: "有效签名放行", status: "Valid", thumbprint: "AA BB CC"},
		{name: "签名校验失败拒绝", status: "HashMismatch", wantErr: true},
		{name: "签名不可信拒绝", status: "NotTrusted", wantErr: true},
		{name: "无法识别签名默认放行", status: "UnknownError"},
		{name: "无法识别签名时要求签名拒绝", status: "UnknownError", required: true, wantErr: true},
		{name: "指定指纹且匹配放行", status: "Valid", thumbprint: "aabbcc", pinned: []string{"AA:BB:CC"}},
		{name: "指定指纹但不匹配拒绝", status: "Valid", thumbprint: "aabbcc", pinned: []string{"DDEEFF"}, wantErr: true},
		{name: "指定指纹时未签名拒绝", status: "NotSigned", pinned: []string{"AABBCC"}, wantErr: true},
		{name: "自签名证书指纹匹配放行", status: "UnknownError", thumbprint: "aabbcc", pinned: []string{"AABBCC"}},
		{name: "根不受信但指纹匹配放行", status: "NotTrusted", thumbprint: "aabbcc", pinned: []string{"AABBCC"}},
		{name: "自签名但没有指纹拒绝", status: "UnknownError", pinned: []string{"AABBCC"}, wantErr: true},
		{name: "篡改状态即使指纹相同也拒绝", status: "HashMismatch", thumbprint: "aabbcc", pinned: []string{"AABBCC"}, wantErr: true},
	}
	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			err := signaturePolicy(SignatureInfo{Status: tt.status, Thumbprint: tt.thumbprint}, tt.required, tt.pinned)
			if tt.wantErr && err == nil {
				t.Fatal("期望拒绝安装，实际放行")
			}
			if !tt.wantErr && err != nil {
				t.Fatalf("期望放行，实际拒绝：%v", err)
			}
		})
	}
}

func TestSignatureThumbprintsFromEnv(t *testing.T) {
	// 未配置环境变量时使用内置信任指纹（已启用代码签名）。
	t.Setenv("AGENT_NOTIFY_SIGNATURE_THUMBPRINT", "")
	got := signatureThumbprintsFromEnv()
	if len(got) != 1 || normalizeThumbprint(got[0]) != normalizeThumbprint(defaultSignatureThumbprint) {
		t.Fatalf("默认应使用内置指纹 %q，得到 %#v", defaultSignatureThumbprint, got)
	}
	// 环境变量覆盖内置指纹。
	t.Setenv("AGENT_NOTIFY_SIGNATURE_THUMBPRINT", "aa bb,CC:DD; ee")
	got = signatureThumbprintsFromEnv()
	if len(got) != 3 || normalizeThumbprint(got[0]) != "AABB" || normalizeThumbprint(got[1]) != "CCDD" || normalizeThumbprint(got[2]) != "EE" {
		t.Fatalf("解析结果不符：%#v", got)
	}
}

// 未签名的安装包在默认策略下会被内置指纹拒绝；强制签名或指定错误指纹同样拒绝。
func TestVerifyArtifactSignatureUnsignedFile(t *testing.T) {
	if runtime.GOOS != "windows" {
		t.Skip("Authenticode 校验仅在 Windows 上可用")
	}
	// 测试二进制本身是未签名的有效 PE，行为等价于未签名的安装包。
	unsigned, err := os.Executable()
	if err != nil {
		t.Fatal(err)
	}

	t.Setenv("AGENT_NOTIFY_REQUIRE_SIGNATURE", "")
	t.Setenv("AGENT_NOTIFY_SIGNATURE_THUMBPRINT", "")
	if err := verifyArtifactSignature(context.Background(), unsigned); err == nil {
		t.Fatal("内置信任指纹生效时，未签名安装包必须被拒绝")
	}

	t.Setenv("AGENT_NOTIFY_REQUIRE_SIGNATURE", "1")
	if err := verifyArtifactSignature(context.Background(), unsigned); err == nil {
		t.Fatal("要求签名时必须拒绝未签名安装包")
	}

	t.Setenv("AGENT_NOTIFY_REQUIRE_SIGNATURE", "")
	t.Setenv("AGENT_NOTIFY_SIGNATURE_THUMBPRINT", "AABBCCDD")
	if err := verifyArtifactSignature(context.Background(), unsigned); err == nil {
		t.Fatal("配置信任指纹时必须拒绝未签名安装包")
	}

	// 非 PE 文件无法校验签名，同样拒绝。
	broken := filepath.Join(t.TempDir(), "broken-installer.exe")
	if err := os.WriteFile(broken, append([]byte("MZ"), make([]byte, 64)...), 0600); err != nil {
		t.Fatal(err)
	}
	t.Setenv("AGENT_NOTIFY_SIGNATURE_THUMBPRINT", "")
	if err := verifyArtifactSignature(context.Background(), broken); err == nil {
		t.Fatal("无法识别的安装包必须拒绝")
	}
}
