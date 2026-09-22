use agentnotify_desktop::update::{
    DEFAULT_SIGNATURE_THUMBPRINT, SignatureRequirement, SignatureStatus, normalize_thumbprint,
    sha256_file, signature_policy, signer_thumbprint, verify_download, verify_executable,
    verify_signature,
};

#[test]
fn updater_rejects_checksum_mismatch_and_unsigned_package_when_required() {
    let file = tempfile::NamedTempFile::new().expect("临时更新包");
    let error = verify_download(
        file.path(),
        "0000000000000000000000000000000000000000000000000000000000000000",
        SignatureRequirement::Required,
        None,
    )
    .unwrap_err();
    assert_eq!(error.code(), "update_checksum_mismatch");
}

#[test]
fn updater_rejects_malformed_sha256_before_opening_the_package() {
    let file = tempfile::NamedTempFile::new().expect("临时更新包");
    let error = verify_download(
        file.path(),
        "not-a-sha256",
        SignatureRequirement::Optional,
        None,
    )
    .unwrap_err();
    assert_eq!(error.code(), "update_hash_invalid");
}

#[test]
fn updater_rejects_non_pe_content_even_when_checksum_matches() {
    let file = tempfile::NamedTempFile::new().expect("临时更新包");
    std::fs::write(file.path(), b"not a windows executable").expect("写入测试文件");
    let hash = sha256_file(file.path()).expect("计算测试哈希");
    let error =
        verify_download(file.path(), &hash, SignatureRequirement::Optional, None).unwrap_err();
    assert_eq!(error.code(), "update_not_pe");
}

#[test]
fn preview_can_accept_an_unsigned_pe_but_formal_channel_rejects_it() {
    let executable = std::env::current_exe().expect("测试可执行文件");
    let hash = sha256_file(&executable).expect("计算测试程序哈希");

    let verified = verify_download(&executable, &hash, SignatureRequirement::Optional, None)
        .expect("预览包允许未签名 PE");
    assert!(!verified.signed);

    let error =
        verify_download(&executable, &hash, SignatureRequirement::Required, None).unwrap_err();
    assert_eq!(error.code(), "update_signature_missing");
}

#[test]
fn updater_rejects_a_pe_with_the_wrong_version() {
    let executable = std::path::PathBuf::from(std::env::var_os("SystemRoot").expect("SystemRoot"))
        .join("System32")
        .join("notepad.exe");
    let hash = sha256_file(&executable).expect("计算系统程序哈希");
    let error = verify_download(
        &executable,
        &hash,
        SignatureRequirement::Optional,
        Some("999.999.999"),
    )
    .unwrap_err();
    assert_eq!(error.code(), "update_version_mismatch");
}

#[test]
fn built_in_signature_thumbprint_matches_the_go_source_of_truth() {
    let go_source = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../internal/update/signature.go"
    ))
    .expect("必须能读取 Go 版签名指纹来源");
    let expected = format!("defaultSignatureThumbprint = \"{DEFAULT_SIGNATURE_THUMBPRINT}\"");
    assert!(
        go_source.contains(&expected),
        "Rust 内置指纹必须与 internal/update/signature.go 一致；轮换证书时两边同步更新"
    );
    assert_eq!(
        normalize_thumbprint(DEFAULT_SIGNATURE_THUMBPRINT),
        DEFAULT_SIGNATURE_THUMBPRINT,
        "内置指纹必须已经是归一化的大写十六进制"
    );
}

#[test]
fn pinned_policy_accepts_only_the_trusted_signer() {
    let pinned = vec![DEFAULT_SIGNATURE_THUMBPRINT.to_owned()];

    // 自签名证书的链不受信（NotTrusted），但指纹匹配时必须放行。
    assert!(
        signature_policy(
            SignatureStatus::NotTrusted,
            DEFAULT_SIGNATURE_THUMBPRINT,
            SignatureRequirement::Required,
            &pinned,
        )
        .expect("指纹匹配必须放行")
    );

    let error = signature_policy(
        SignatureStatus::Valid,
        "AABBCCDD",
        SignatureRequirement::Required,
        &pinned,
    )
    .unwrap_err();
    assert_eq!(error.code(), "update_signature_untrusted");
    assert!(error.message().contains("AABBCCDD"));

    let error = signature_policy(
        SignatureStatus::NotSigned,
        "",
        SignatureRequirement::Required,
        &pinned,
    )
    .unwrap_err();
    assert_eq!(
        error.code(),
        "update_signature_missing",
        "未签名必须在正式通道被明确拒绝"
    );

    let error = signature_policy(
        SignatureStatus::HashMismatch,
        DEFAULT_SIGNATURE_THUMBPRINT,
        SignatureRequirement::Required,
        &pinned,
    )
    .unwrap_err();
    assert_eq!(
        error.code(),
        "update_signature_abnormal",
        "篡改信号即使指纹匹配也必须拒绝"
    );

    // 预览通道显式放宽：即使配置了指纹，也允许未签名包（结果 signed=false）。
    assert!(
        !signature_policy(
            SignatureStatus::NotSigned,
            "",
            SignatureRequirement::Optional,
            &pinned,
        )
        .expect("预览通道允许未签名")
    );
}

#[test]
fn unpinned_policy_requires_a_signature_only_on_the_formal_channel() {
    assert!(
        signature_policy(
            SignatureStatus::Valid,
            "whatever",
            SignatureRequirement::Required,
            &[],
        )
        .expect("有效签名必须放行")
    );
    assert!(
        !signature_policy(
            SignatureStatus::NotSigned,
            "",
            SignatureRequirement::Optional,
            &[],
        )
        .expect("预览通道允许未签名")
    );

    let error = signature_policy(
        SignatureStatus::NotSigned,
        "",
        SignatureRequirement::Required,
        &[],
    )
    .unwrap_err();
    assert_eq!(error.code(), "update_signature_missing");

    let error = signature_policy(
        SignatureStatus::NotTrusted,
        "whatever",
        SignatureRequirement::Optional,
        &[],
    )
    .unwrap_err();
    assert_eq!(error.code(), "update_signature_invalid");
}

#[test]
fn verify_executable_checks_pe_and_signature_without_a_release_checksum() {
    let file = tempfile::NamedTempFile::new().expect("临时文件");
    std::fs::write(file.path(), b"not a windows executable").expect("写入测试文件");
    let error = verify_executable(file.path(), SignatureRequirement::Optional, None).unwrap_err();
    assert_eq!(error.code(), "update_not_pe");
}

#[test]
fn pinned_formal_channel_reads_the_embedded_signature_and_rejects_unknown_signers() {
    let dir = tempfile::Builder::new()
        .prefix("agentnotify-update-sign-")
        .tempdir_in(agentnotify_testkit::test_temp_root())
        .expect("测试临时目录必须可创建");
    let signed = dir.path().join("signed-test.exe");
    let script = dir.path().join("sign-test.ps1");
    std::fs::write(&script, SIGN_TEST_SCRIPT).expect("写入签名脚本");

    let source = std::path::PathBuf::from(std::env::var_os("SystemRoot").expect("SystemRoot"))
        .join("System32")
        .join("notepad.exe");
    let output = std::process::Command::new("powershell.exe")
        .args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-File"])
        .arg(&script)
        .arg(&signed)
        .arg(&source)
        .output()
        .expect("必须能运行 PowerShell");
    assert!(
        output.status.success(),
        "自签名测试程序必须生成成功：{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let expected = String::from_utf8_lossy(&output.stdout)
        .trim()
        .to_ascii_uppercase();
    assert_eq!(expected.len(), 40, "证书指纹必须是 40 位十六进制");

    let extracted = signer_thumbprint(&signed).expect("必须能从 PE 内嵌签名读出签名者指纹");
    assert_eq!(extracted, expected, "读出的指纹必须等于签名证书指纹");

    // 指纹匹配时（自签名、链不受信）正式通道必须放行，且结果标记为已签名。
    assert!(
        signature_policy(
            SignatureStatus::NotTrusted,
            &extracted,
            SignatureRequirement::Required,
            std::slice::from_ref(&extracted),
        )
        .expect("指纹匹配必须放行")
    );
    // 内置信任列表里没有这张测试证书：正式通道必须拒绝，并说明签名者不匹配。
    let error = verify_signature(&signed, SignatureRequirement::Required).unwrap_err();
    assert_eq!(error.code(), "update_signature_untrusted");
    assert!(
        error.message().contains("签名者不匹配"),
        "{}",
        error.message()
    );
}

/// 用自签名证书给一个真实 PE 文件签名并输出证书指纹；脚本必须保持纯 ASCII。
const SIGN_TEST_SCRIPT: &str = r#"param([string]$Destination, [string]$Source)
$ErrorActionPreference = 'Stop'
$cert = New-SelfSignedCertificate -Type CodeSigningCert -Subject 'CN=AgentNotifyUpdaterTest' -CertStoreLocation 'Cert:\CurrentUser\My' -NotAfter (Get-Date).AddDays(1)
try {
  Copy-Item -LiteralPath $Source -Destination $Destination -Force
  $result = Set-AuthenticodeSignature -LiteralPath $Destination -Certificate $cert
  if ($result.Status -ne 'Valid' -and $result.Status -ne 'UnknownError' -and $result.Status -ne 'NotTrusted') { throw "unexpected signature status: $($result.Status)" }
  [Console]::Out.Write($cert.Thumbprint)
} finally {
  Remove-Item -LiteralPath ("Cert:\CurrentUser\My\" + $cert.Thumbprint) -Force -ErrorAction SilentlyContinue
}
"#;
