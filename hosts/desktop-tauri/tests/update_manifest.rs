use std::{
    fs,
    path::Path,
    time::{Duration, SystemTime},
};

use agentnotify_desktop::update::{
    SignatureRequirement, VerifiedReleaseManifest, verify_release_manifest_at,
};

// 夹具证书由临时自签证书生成（仅用于测试，私钥不入库），换签名时必须同步此指纹。
const THUMBPRINT: &str = "50343302D62028C5315FCFEE67C063CD759C2009";
const VERSION: &[u8] = include_bytes!("fixtures/release-manifest/VERSION");
const PLUGIN: &[u8] = include_bytes!("fixtures/release-manifest/plugin/agent-notify.ts");
const HOOK: &[u8] = include_bytes!("fixtures/release-manifest/tools/hooks/install-opencode-v2.ps1");
const MANIFEST: &[u8] = include_bytes!("fixtures/release-manifest/RELEASE-MANIFEST.json");
const SIGNATURE: &[u8] = include_bytes!("fixtures/release-manifest/RELEASE-MANIFEST.p7s");
const SHA384_SIGNATURE: &[u8] =
    include_bytes!("fixtures/release-manifest-negative/RELEASE-MANIFEST.sha384.p7s");
const MULTI_SIGNATURE: &[u8] =
    include_bytes!("fixtures/release-manifest-negative/RELEASE-MANIFEST.multi.p7s");

fn test_dir(prefix: &str) -> tempfile::TempDir {
    tempfile::Builder::new()
        .prefix(prefix)
        .tempdir_in(agentnotify_testkit::test_temp_root())
        .expect("测试临时目录必须可创建")
}

fn copy_fixture(root: &Path) {
    fs::create_dir_all(root.join("plugin")).expect("创建插件目录");
    fs::create_dir_all(root.join("tools/hooks")).expect("创建 Hook 目录");
    fs::write(root.join("VERSION"), VERSION).expect("写入版本文件");
    fs::write(root.join("plugin/agent-notify.ts"), PLUGIN).expect("写入插件文件");
    fs::write(root.join("tools/hooks/install-opencode-v2.ps1"), HOOK).expect("写入 Hook 文件");
    fs::write(root.join("RELEASE-MANIFEST.json"), MANIFEST).expect("写入发布清单");
    fs::write(root.join("RELEASE-MANIFEST.p7s"), SIGNATURE).expect("写入发布清单签名");
}

fn trusted() -> Vec<String> {
    vec![THUMBPRINT.to_owned()]
}

fn fixed_now() -> SystemTime {
    SystemTime::UNIX_EPOCH + Duration::from_secs(1_800_000_000)
}

#[test]
fn beta_allows_a_legacy_archive_only_when_both_controls_are_missing() {
    let dir = test_dir("agentnotify-manifest-legacy-");
    let root = dir.path();
    copy_fixture(root);
    fs::remove_file(root.join("RELEASE-MANIFEST.json")).expect("删除清单");
    fs::remove_file(root.join("RELEASE-MANIFEST.p7s")).expect("删除签名");

    assert_eq!(
        verify_release_manifest_at(
            root,
            "2.0.7",
            SignatureRequirement::Optional,
            fixed_now(),
            &trusted(),
        )
        .expect("Beta 应允许旧开发包"),
        VerifiedReleaseManifest::Legacy
    );
    let error = verify_release_manifest_at(
        root,
        "2.0.7",
        SignatureRequirement::Required,
        fixed_now(),
        &trusted(),
    )
    .expect_err("Stable 不得允许旧开发包");
    assert_eq!(error.code(), "update_manifest_missing");
}

#[test]
fn beta_does_not_turn_a_missing_root_into_a_legacy_archive() {
    let dir = test_dir("agentnotify-manifest-missing-root-");
    let missing_root = dir.path().join("missing");
    let error = verify_release_manifest_at(
        &missing_root,
        "2.0.7",
        SignatureRequirement::Optional,
        fixed_now(),
        &trusted(),
    )
    .expect_err("不存在的根目录不得被当成旧开发包");
    assert_eq!(error.code(), "update_manifest_invalid");
}

#[test]
fn a_single_missing_control_file_is_always_rejected() {
    let dir = test_dir("agentnotify-manifest-half-control-");
    let root = dir.path();
    copy_fixture(root);
    fs::remove_file(root.join("RELEASE-MANIFEST.p7s")).expect("删除签名");

    let error = verify_release_manifest_at(
        root,
        "2.0.7",
        SignatureRequirement::Optional,
        fixed_now(),
        &trusted(),
    )
    .expect_err("半缺失控制文件不得降级");
    assert_eq!(error.code(), "update_manifest_signature_missing");
}

#[cfg(windows)]
#[test]
fn a_valid_signed_manifest_authenticates_every_payload_file() {
    let dir = test_dir("agentnotify-manifest-valid-");
    let root = dir.path();
    copy_fixture(root);

    let verified = verify_release_manifest_at(
        root,
        "2.0.7",
        SignatureRequirement::Required,
        fixed_now(),
        &trusted(),
    )
    .expect("合法签名清单必须通过");
    let VerifiedReleaseManifest::Signed { files } = verified else {
        panic!("正式清单必须返回签名文件列表");
    };
    assert_eq!(files.len(), 3);
}

#[cfg(windows)]
#[test]
fn a_tampered_manifest_or_payload_is_rejected_before_install() {
    let dir = test_dir("agentnotify-manifest-tamper-");
    let root = dir.path();
    copy_fixture(root);
    let manifest_path = root.join("RELEASE-MANIFEST.json");
    let mut tampered_manifest = MANIFEST.to_vec();
    tampered_manifest.push(b' ');
    fs::write(&manifest_path, tampered_manifest).expect("篡改清单");
    let error = verify_release_manifest_at(
        root,
        "2.0.7",
        SignatureRequirement::Required,
        fixed_now(),
        &trusted(),
    )
    .expect_err("篡改清单必须拒绝");
    assert_eq!(error.code(), "update_manifest_signature_invalid");

    copy_fixture(root);
    fs::write(root.join("VERSION"), b"2.0.8").expect("篡改版本文件");
    let error = verify_release_manifest_at(
        root,
        "2.0.7",
        SignatureRequirement::Required,
        fixed_now(),
        &trusted(),
    )
    .expect_err("篡改文件必须拒绝");
    assert_eq!(error.code(), "update_manifest_hash_mismatch");
}

#[cfg(windows)]
#[test]
fn extra_or_missing_payload_files_are_rejected() {
    let dir = test_dir("agentnotify-manifest-file-set-");
    let root = dir.path();
    copy_fixture(root);
    fs::write(root.join("unexpected.dll"), b"not-in-manifest").expect("写入额外文件");
    let error = verify_release_manifest_at(
        root,
        "2.0.7",
        SignatureRequirement::Required,
        fixed_now(),
        &trusted(),
    )
    .expect_err("额外文件必须拒绝");
    assert_eq!(error.code(), "update_manifest_file_extra");

    fs::remove_file(root.join("unexpected.dll")).expect("删除额外文件");
    copy_fixture(root);
    fs::remove_file(root.join("plugin/agent-notify.ts")).expect("删除插件文件");
    let error = verify_release_manifest_at(
        root,
        "2.0.7",
        SignatureRequirement::Required,
        fixed_now(),
        &trusted(),
    )
    .expect_err("缺失文件必须拒绝");
    assert_eq!(error.code(), "update_manifest_file_missing");
}

#[cfg(windows)]
#[test]
fn non_sha256_and_multiple_signers_are_rejected_before_install() {
    let dir = test_dir("agentnotify-manifest-negative-signatures-");
    let root = dir.path();
    copy_fixture(root);
    fs::write(root.join("RELEASE-MANIFEST.p7s"), SHA384_SIGNATURE).expect("替换为非 SHA-256 签名");
    let error = verify_release_manifest_at(
        root,
        "2.0.7",
        SignatureRequirement::Required,
        fixed_now(),
        &trusted(),
    )
    .expect_err("非 SHA-256 签名必须拒绝");
    assert_eq!(error.code(), "update_manifest_signature_invalid");

    fs::write(root.join("RELEASE-MANIFEST.p7s"), MULTI_SIGNATURE).expect("替换为多签名者签名");
    let error = verify_release_manifest_at(
        root,
        "2.0.7",
        SignatureRequirement::Required,
        fixed_now(),
        &trusted(),
    )
    .expect_err("多签名者必须拒绝");
    assert_eq!(error.code(), "update_manifest_signature_invalid");
}

#[cfg(windows)]
#[test]
fn wrong_signer_and_invalid_certificate_time_are_rejected() {
    let dir = test_dir("agentnotify-manifest-trust-");
    let root = dir.path();
    copy_fixture(root);
    let error = verify_release_manifest_at(
        root,
        "2.0.7",
        SignatureRequirement::Required,
        fixed_now(),
        &["0000000000000000000000000000000000000000".to_owned()],
    )
    .expect_err("错误签名者必须拒绝");
    assert_eq!(error.code(), "update_manifest_signature_untrusted");

    let before = verify_release_manifest_at(
        root,
        "2.0.7",
        SignatureRequirement::Required,
        SystemTime::UNIX_EPOCH,
        &trusted(),
    )
    .expect_err("证书生效前必须拒绝");
    assert_eq!(before.code(), "update_manifest_expired");
    let after = verify_release_manifest_at(
        root,
        "2.0.7",
        SignatureRequirement::Required,
        SystemTime::UNIX_EPOCH + Duration::from_secs(4_000_000_000),
        &trusted(),
    )
    .expect_err("证书过期后必须拒绝");
    assert_eq!(after.code(), "update_manifest_expired");
}
