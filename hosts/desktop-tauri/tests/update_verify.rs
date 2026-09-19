use agentnotify_desktop::update::{SignatureRequirement, sha256_file, verify_download};

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
