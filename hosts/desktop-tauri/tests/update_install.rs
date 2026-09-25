use std::{
    ffi::OsString,
    fs,
    io::Write,
    path::{Path, PathBuf},
    sync::Mutex,
    time::{Duration, SystemTime},
};

use agentnotify_desktop::update::{
    AppExitRequester, HttpTextResponse, InstallMode, InstallerLaunchOutcome,
    InstallerLaunchRequest, InstallerLauncher, MAX_EXTRACTED_BYTES, SignatureRequirement,
    UpdateChannel, UpdateConfig, UpdateError, UpdateService, UpdateTransport, apply_staged_release,
    extract_archive, installer_arguments, launch_installer, safe_entry_path,
    validate_staged_release, validate_staged_release_with_manifest, within_extraction_budget,
};

const EXPECTED_INSTALLER_ARGS: [&str; 3] = ["/SILENT", "/NORESTART", "/LOG="];

/// 假启动器：记录请求并返回预置结果，测试不真的运行安装程序。
#[derive(Default)]
struct FakeLauncher {
    outcome: Mutex<Option<Result<InstallerLaunchOutcome, UpdateError>>>,
    requests: Mutex<Vec<InstallerLaunchRequest>>,
}

impl FakeLauncher {
    fn with_outcome(outcome: InstallerLaunchOutcome) -> Self {
        Self {
            outcome: Mutex::new(Some(Ok(outcome))),
            requests: Mutex::new(Vec::new()),
        }
    }

    /// 启动失败（例如权限或路径问题）：不拉起任何进程。
    fn failing(error: UpdateError) -> Self {
        Self {
            outcome: Mutex::new(Some(Err(error))),
            requests: Mutex::new(Vec::new()),
        }
    }

    fn requests(&self) -> Vec<InstallerLaunchRequest> {
        self.requests.lock().expect("请求记录锁").clone()
    }
}

#[async_trait::async_trait]
impl InstallerLauncher for FakeLauncher {
    async fn launch(
        &self,
        request: InstallerLaunchRequest,
    ) -> Result<InstallerLaunchOutcome, UpdateError> {
        self.requests.lock().expect("请求记录锁").push(request);
        self.outcome
            .lock()
            .expect("结果锁")
            .take()
            .unwrap_or(Ok(InstallerLaunchOutcome::StillRunning))
    }
}

/// 假退出端口：只记录"请求应用退出"的次数，测试绝不真的退出进程。
#[derive(Default)]
struct FakeExitRequester {
    requested: Mutex<usize>,
}

impl FakeExitRequester {
    fn requested(&self) -> usize {
        *self.requested.lock().expect("退出请求锁")
    }
}

impl AppExitRequester for FakeExitRequester {
    fn request_exit(&self) {
        *self.requested.lock().expect("退出请求锁") += 1;
    }
}

fn test_dir(prefix: &str) -> tempfile::TempDir {
    tempfile::Builder::new()
        .prefix(prefix)
        .tempdir_in(agentnotify_testkit::test_temp_root())
        .expect("测试临时目录必须可创建")
}

fn write_zip(path: &Path, entries: &[(&str, &[u8])]) {
    let file = fs::File::create(path).expect("创建测试 ZIP");
    let mut writer = zip::ZipWriter::new(file);
    let options =
        zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    for (name, contents) in entries {
        writer.start_file(*name, options).expect("写入 ZIP 条目");
        writer.write_all(contents).expect("写入 ZIP 内容");
    }
    writer.finish().expect("完成测试 ZIP");
}

fn write_staged_release_files(staging: &Path, version: &str) -> PathBuf {
    let root = staging.join("Agent-notify");
    fs::create_dir_all(root.join("bin")).expect("创建测试发布目录");
    fs::write(root.join("VERSION"), version).expect("写入版本文件");
    fs::write(
        root.join("bin").join("agentnotify-desktop.exe"),
        b"new-binary",
    )
    .expect("写入主程序");
    root
}

fn build_staged_release(
    staging: &Path,
    version: &str,
) -> agentnotify_desktop::update::StagedRelease {
    write_staged_release_files(staging, version);
    validate_staged_release_with_manifest(
        staging,
        version,
        SignatureRequirement::Optional,
        SystemTime::UNIX_EPOCH + Duration::from_secs(1_800_000_000),
        &[],
    )
    .expect("测试发布目录必须合法")
}

#[test]
fn installer_arguments_match_the_go_baseline() {
    let log_path = agentnotify_testkit::test_temp_root()
        .join("agentnotify")
        .join("updates")
        .join("last-update.log");
    let install_dir = OsString::from("/DIR=D:\\Apps\\Agent-notify");
    let args = installer_arguments(&log_path, &[install_dir]);
    let values: Vec<String> = args
        .iter()
        .map(|value| value.to_string_lossy().into_owned())
        .collect();
    assert_eq!(values[0], "/SILENT");
    assert_eq!(values[1], "/NORESTART");
    assert_eq!(values[2], format!("/LOG={}", log_path.display()));
    assert_eq!(values[3], "/DIR=D:\\Apps\\Agent-notify");
}

#[tokio::test]
async fn launch_installer_rejects_a_missing_installer_without_starting_anything() {
    let dir = test_dir("agentnotify-install-missing-");
    let launcher = FakeLauncher::default();
    let error = launch_installer(
        &dir.path().join("not-there.exe"),
        &dir.path().join("last-update.log"),
        &[],
        &launcher,
    )
    .await
    .unwrap_err();

    assert_eq!(error.code(), "update_installer_missing");
    assert!(launcher.requests().is_empty(), "缺失安装器不得启动进程");
}

#[tokio::test]
async fn launch_installer_starts_a_silent_install_with_the_expected_arguments() {
    let dir = test_dir("agentnotify-install-launch-");
    let installer = dir.path().join("Agent-notify-Setup-v2.1.0.exe");
    fs::write(&installer, b"MZ-fake").expect("写入假安装器");
    let log_path = dir.path().join("logs").join("last-update.log");
    let launcher = FakeLauncher::default();

    launch_installer(&installer, &log_path, &[], &launcher)
        .await
        .expect("安装器必须成功启动");

    assert!(
        log_path.parent().expect("日志目录").is_dir(),
        "必须创建日志目录"
    );
    let requests = launcher.requests();
    assert_eq!(requests.len(), 1);
    let request = &requests[0];
    assert_eq!(request.program, installer);
    assert_eq!(request.working_dir, dir.path());
    let values: Vec<String> = request
        .args
        .iter()
        .map(|value| value.to_string_lossy().into_owned())
        .collect();
    assert_eq!(values.len(), EXPECTED_INSTALLER_ARGS.len());
    assert_eq!(values[0], EXPECTED_INSTALLER_ARGS[0]);
    assert_eq!(values[1], EXPECTED_INSTALLER_ARGS[1]);
    assert_eq!(
        values[2],
        format!("{}{}", EXPECTED_INSTALLER_ARGS[2], log_path.display())
    );
}

#[tokio::test]
async fn launch_installer_reports_an_immediate_nonzero_exit_with_the_log_path() {
    let dir = test_dir("agentnotify-install-early-exit-");
    let installer = dir.path().join("Agent-notify-Setup-v2.1.0.exe");
    fs::write(&installer, b"MZ-fake").expect("写入假安装器");
    let log_path = dir.path().join("last-update.log");
    let launcher = FakeLauncher::with_outcome(InstallerLaunchOutcome::Exited { code: Some(5) });

    let error = launch_installer(&installer, &log_path, &[], &launcher)
        .await
        .unwrap_err();

    assert_eq!(error.code(), "update_installer_failed");
    assert!(error.message().contains("退出码 5"), "{}", error.message());
    assert!(
        error.message().contains("last-update.log"),
        "{}",
        error.message()
    );
}

#[tokio::test]
async fn launch_installer_accepts_a_zero_exit_installer() {
    let dir = test_dir("agentnotify-install-zero-exit-");
    let installer = dir.path().join("Agent-notify-Setup-v2.1.0.exe");
    fs::write(&installer, b"MZ-fake").expect("写入假安装器");
    let launcher = FakeLauncher::with_outcome(InstallerLaunchOutcome::Exited { code: Some(0) });

    launch_installer(
        &installer,
        &dir.path().join("last-update.log"),
        &[],
        &launcher,
    )
    .await
    .expect("退出码 0 视为安装器瞬间完成");
}

#[test]
fn safe_entry_path_rejects_traversal_and_absolute_paths() {
    for name in [
        "../evil.txt",
        "a/../../evil.txt",
        "..",
        "a/..",
        "\\evil.txt",
        "C:/evil.txt",
        "C:evil.txt",
        "//server/share/evil.txt",
        "",
    ] {
        let error = safe_entry_path(name).unwrap_err();
        assert_eq!(
            error.code(),
            "update_archive_path_unsafe",
            "必须拒绝越界路径 {name:?}"
        );
    }

    assert_eq!(
        safe_entry_path("Agent-notify\\bin\\agentnotify-desktop.exe").expect("合法路径"),
        PathBuf::from("Agent-notify")
            .join("bin")
            .join("agentnotify-desktop.exe")
    );
    assert_eq!(
        safe_entry_path("Agent-notify/./bin/../VERSION").expect("合法路径"),
        PathBuf::from("Agent-notify").join("VERSION")
    );
}

#[test]
fn extraction_budget_rejects_overflowing_declared_sizes() {
    let max = MAX_EXTRACTED_BYTES;
    assert!(within_extraction_budget(0, 0, max));
    assert!(within_extraction_budget(max, 0, max));
    assert!(within_extraction_budget(0, max, max));
    assert!(!within_extraction_budget(max + 1, 0, max));
    assert!(!within_extraction_budget(1, max, max));
    assert!(
        !within_extraction_budget(u64::MAX, 1, max),
        "溢出不得绕过预算"
    );
    assert!(!within_extraction_budget(0, max + 1, max));
}

#[test]
fn extract_archive_writes_the_release_layout_into_the_destination() {
    let dir = test_dir("agentnotify-install-extract-");
    let archive = dir.path().join("Agent-notify-v2.1.0.zip");
    write_zip(
        &archive,
        &[
            ("Agent-notify/VERSION", b"2.1.0"),
            ("Agent-notify/bin/agentnotify-desktop.exe", b"MZ-binary"),
        ],
    );
    let destination = dir.path().join("extracted");

    extract_archive(&archive, &destination).expect("解包必须成功");

    assert_eq!(
        fs::read(destination.join("Agent-notify").join("VERSION")).expect("版本文件"),
        b"2.1.0"
    );
    assert_eq!(
        fs::read(
            destination
                .join("Agent-notify")
                .join("bin")
                .join("agentnotify-desktop.exe")
        )
        .expect("主程序"),
        b"MZ-binary"
    );
}

#[test]
fn extract_archive_rejects_duplicate_entries() {
    let dir = test_dir("agentnotify-install-duplicate-");
    let archive = dir.path().join("duplicate.zip");
    write_zip(
        &archive,
        &[
            ("Agent-notify/VERSION", b"2.1.0"),
            ("Agent-notify/./VERSION", b"2.1.1"),
        ],
    );
    let destination = dir.path().join("extracted");

    let error = extract_archive(&archive, &destination).unwrap_err();

    assert_eq!(error.code(), "update_archive_path_unsafe");
    assert!(error.message().contains("重复路径"));
}

#[test]
fn extract_archive_rejects_zip_slip_without_writing_outside_the_destination() {
    let dir = test_dir("agentnotify-install-slip-");
    let archive = dir.path().join("evil.zip");
    write_zip(&archive, &[("../evil.txt", b"owned")]);
    let destination = dir.path().join("extracted");

    let error = extract_archive(&archive, &destination).unwrap_err();

    assert_eq!(error.code(), "update_archive_path_unsafe");
    assert!(
        !dir.path().join("evil.txt").exists(),
        "越界文件绝不能落到解压目录之外"
    );
}

#[test]
fn extract_archive_rejects_symlink_entries() {
    let dir = test_dir("agentnotify-install-symlink-");
    let archive = dir.path().join("symlink.zip");
    // zip crate 的写入 API 会把 S_IFLNK 位屏蔽成 0o777，这里手工构造带符号链接属性的条目。
    write_symlink_zip(&archive, "Agent-notify/link");
    let destination = dir.path().join("extracted");

    let error = extract_archive(&archive, &destination).unwrap_err();

    assert_eq!(error.code(), "update_archive_symlink");
    assert!(!destination.join("Agent-notify").join("link").exists());
}

/// 手工构造只含一个符号链接条目的最小 ZIP（stored，无 CRC 校验需求）。
fn write_symlink_zip(path: &Path, name: &str) {
    const LOCAL_HEADER: u32 = 0x0403_4b50;
    const CENTRAL_HEADER: u32 = 0x0201_4b50;
    const END_OF_CENTRAL: u32 = 0x0605_4b50;
    const UNIX_MADE_BY: u16 = 0x031E;
    const SYMLINK_ATTRIBUTES: u32 = 0o120777 << 16;

    let name = name.as_bytes();
    let contents = b"target";
    let size = contents.len() as u32;
    let mut bytes = Vec::new();

    bytes.extend_from_slice(&LOCAL_HEADER.to_le_bytes());
    bytes.extend_from_slice(&20u16.to_le_bytes());
    bytes.extend_from_slice(&0u16.to_le_bytes());
    bytes.extend_from_slice(&0u16.to_le_bytes());
    bytes.extend_from_slice(&0u16.to_le_bytes());
    bytes.extend_from_slice(&0u16.to_le_bytes());
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes.extend_from_slice(&size.to_le_bytes());
    bytes.extend_from_slice(&size.to_le_bytes());
    bytes.extend_from_slice(&(name.len() as u16).to_le_bytes());
    bytes.extend_from_slice(&0u16.to_le_bytes());
    bytes.extend_from_slice(name);
    bytes.extend_from_slice(contents);

    let central_offset = bytes.len() as u32;
    bytes.extend_from_slice(&CENTRAL_HEADER.to_le_bytes());
    bytes.extend_from_slice(&UNIX_MADE_BY.to_le_bytes());
    bytes.extend_from_slice(&20u16.to_le_bytes());
    bytes.extend_from_slice(&0u16.to_le_bytes());
    bytes.extend_from_slice(&0u16.to_le_bytes());
    bytes.extend_from_slice(&0u16.to_le_bytes());
    bytes.extend_from_slice(&0u16.to_le_bytes());
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes.extend_from_slice(&size.to_le_bytes());
    bytes.extend_from_slice(&size.to_le_bytes());
    bytes.extend_from_slice(&(name.len() as u16).to_le_bytes());
    bytes.extend_from_slice(&0u16.to_le_bytes());
    bytes.extend_from_slice(&0u16.to_le_bytes());
    bytes.extend_from_slice(&0u16.to_le_bytes());
    bytes.extend_from_slice(&0u16.to_le_bytes());
    bytes.extend_from_slice(&SYMLINK_ATTRIBUTES.to_le_bytes());
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes.extend_from_slice(name);

    let central_size = bytes.len() as u32 - central_offset;
    bytes.extend_from_slice(&END_OF_CENTRAL.to_le_bytes());
    bytes.extend_from_slice(&0u16.to_le_bytes());
    bytes.extend_from_slice(&0u16.to_le_bytes());
    bytes.extend_from_slice(&1u16.to_le_bytes());
    bytes.extend_from_slice(&1u16.to_le_bytes());
    bytes.extend_from_slice(&central_size.to_le_bytes());
    bytes.extend_from_slice(&central_offset.to_le_bytes());
    bytes.extend_from_slice(&0u16.to_le_bytes());

    fs::write(path, bytes).expect("写入手工构造的 ZIP");
}

#[test]
fn staged_release_validation_requires_layout_and_matching_version() {
    let dir = test_dir("agentnotify-install-validate-");
    let staging = dir.path().join("staging");

    let error = validate_staged_release(&staging, "2.1.0").unwrap_err();
    assert_eq!(error.code(), "update_release_layout_invalid");

    let root = staging.join("Agent-notify");
    fs::create_dir_all(root.join("bin")).expect("创建发布目录");
    fs::write(root.join("VERSION"), "2.1.0").expect("写入版本文件");
    let error = validate_staged_release(&staging, "2.1.0").unwrap_err();
    assert_eq!(error.code(), "update_release_layout_invalid");
    assert!(error.message().contains("agentnotify-desktop.exe"));

    fs::write(root.join("bin").join("agentnotify-desktop.exe"), b"MZ").expect("写入主程序");
    fs::write(root.join("VERSION"), "2.0.9").expect("写入错误版本");
    let error = validate_staged_release(&staging, "2.1.0").unwrap_err();
    assert_eq!(error.code(), "update_release_version_mismatch");

    fs::write(root.join("VERSION"), "2.1.0").expect("写入正确版本");
    validate_staged_release(&staging, "2.1.0").expect("合法发布目录必须通过");
}

#[test]
fn stable_manifest_validation_rejects_a_legacy_layout_before_install() {
    let dir = test_dir("agentnotify-install-manifest-required-");
    let staging = dir.path().join("staging");
    let root = staging.join("Agent-notify");
    fs::create_dir_all(root.join("bin")).expect("创建测试发布目录");
    fs::write(root.join("VERSION"), "2.1.0").expect("写入版本文件");
    fs::write(root.join("bin").join("agentnotify-desktop.exe"), b"MZ").expect("写入主程序");

    let error = validate_staged_release_with_manifest(
        &staging,
        "2.1.0",
        SignatureRequirement::Required,
        SystemTime::UNIX_EPOCH + Duration::from_secs(1_800_000_000),
        &[],
    )
    .expect_err("Stable 必须在替换文件前拒绝无清单 ZIP");

    assert_eq!(error.code(), "update_manifest_missing");
}

#[test]
fn install_relative_path_moves_bin_files_to_the_install_root() {
    assert_eq!(
        agentnotify_desktop::update::install_relative_path(Path::new(
            "bin/agentnotify-desktop.exe"
        )),
        Some(PathBuf::from("agentnotify-desktop.exe"))
    );
    assert_eq!(
        agentnotify_desktop::update::install_relative_path(Path::new("plugin/agent-notify.ts")),
        Some(PathBuf::from("plugin").join("agent-notify.ts"))
    );
    assert_eq!(
        agentnotify_desktop::update::install_relative_path(Path::new("VERSION")),
        Some(PathBuf::from("VERSION"))
    );
    assert_eq!(
        agentnotify_desktop::update::install_relative_path(Path::new("bin")),
        None,
        "空的 bin 目录不产生目标文件"
    );
}

#[test]
fn apply_staged_release_replaces_files_and_keeps_a_backup() {
    let dir = test_dir("agentnotify-install-apply-");
    let install_root = dir.path().join("install");
    fs::create_dir_all(&install_root).expect("创建安装目录");
    fs::write(install_root.join("agentnotify-desktop.exe"), b"old-binary").expect("写入旧程序");
    fs::write(install_root.join("keep.txt"), b"keep").expect("写入无关文件");

    let staging = dir.path().join("staging");
    let root = write_staged_release_files(&staging, "2.1.0");
    fs::create_dir_all(root.join("plugin")).expect("创建插件目录");
    fs::write(root.join("plugin").join("agent-notify.ts"), b"plugin").expect("写入插件文件");
    let staged = validate_staged_release_with_manifest(
        &staging,
        "2.1.0",
        SignatureRequirement::Optional,
        SystemTime::UNIX_EPOCH + Duration::from_secs(1_800_000_000),
        &[],
    )
    .expect("测试发布目录必须合法");

    let backup_dir = dir.path().join("backup");
    let applied = apply_staged_release(&staged, &install_root, &backup_dir).expect("替换必须成功");

    assert_eq!(applied.replaced.len(), 3);
    assert_eq!(
        fs::read(install_root.join("agentnotify-desktop.exe")).expect("新程序"),
        b"new-binary",
        "ZIP 里的 bin/ 程序必须落到安装根目录（与安装器布局一致）"
    );
    assert_eq!(
        fs::read(install_root.join("plugin").join("agent-notify.ts")).expect("新插件"),
        b"plugin"
    );
    assert_eq!(
        fs::read(install_root.join("VERSION")).expect("版本文件"),
        b"2.1.0"
    );
    assert_eq!(
        fs::read(install_root.join("keep.txt")).expect("无关文件"),
        b"keep",
        "替换不得删除安装目录里的其它文件"
    );
    assert_eq!(
        fs::read(backup_dir.join("agentnotify-desktop.exe")).expect("备份文件"),
        b"old-binary",
        "覆盖前必须留下可恢复的备份"
    );
}

#[test]
fn apply_staged_release_only_copies_the_verified_file_list() {
    let dir = test_dir("agentnotify-install-verified-files-");
    let install_root = dir.path().join("install");
    fs::create_dir_all(&install_root).expect("创建安装目录");

    let staging = dir.path().join("staging");
    let staged = build_staged_release(&staging, "2.1.0");
    fs::write(staged.root.join("unlisted.dll"), b"must not install").expect("写入未声明文件");
    fs::write(staged.root.join("RELEASE-MANIFEST.json"), b"not copied")
        .expect("写入未声明清单控制文件");
    fs::write(staged.root.join("RELEASE-MANIFEST.p7s"), b"not copied")
        .expect("写入未声明签名控制文件");

    let backup_dir = dir.path().join("backup");
    apply_staged_release(&staged, &install_root, &backup_dir).expect("已验证文件必须可以替换");

    assert!(!install_root.join("unlisted.dll").exists());
    assert!(!install_root.join("RELEASE-MANIFEST.json").exists());
}

#[test]
fn apply_staged_release_rolls_back_when_a_file_cannot_be_replaced() {
    let dir = test_dir("agentnotify-install-rollback-");
    let install_root = dir.path().join("install");
    fs::create_dir_all(&install_root).expect("创建安装目录");
    fs::write(install_root.join("agentnotify-desktop.exe"), b"old-binary").expect("写入旧程序");
    // 目标位置是同名目录：写入必然失败，用来验证回滚。
    fs::create_dir_all(install_root.join("conflict.txt")).expect("创建冲突目录");

    let staging = dir.path().join("staging");
    let root = write_staged_release_files(&staging, "2.1.0");
    fs::write(root.join("conflict.txt"), b"new-file").expect("写入冲突文件");
    let staged = validate_staged_release_with_manifest(
        &staging,
        "2.1.0",
        SignatureRequirement::Optional,
        SystemTime::UNIX_EPOCH + Duration::from_secs(1_800_000_000),
        &[],
    )
    .expect("测试发布目录必须合法");

    let backup_dir = dir.path().join("backup");
    let error = apply_staged_release(&staged, &install_root, &backup_dir).expect_err("必须失败");

    assert_eq!(error.code(), "update_install_failed");
    assert_eq!(
        fs::read(install_root.join("agentnotify-desktop.exe")).expect("旧程序"),
        b"old-binary",
        "失败后必须恢复旧版本文件"
    );
    assert!(
        install_root.join("conflict.txt").is_dir(),
        "失败后不得破坏原有目录结构"
    );
    assert!(
        !backup_dir.join("agentnotify-desktop.exe").exists(),
        "回滚后备份必须已经还原"
    );
}

#[test]
fn apply_staged_release_rejects_a_missing_install_root() {
    let dir = test_dir("agentnotify-install-root-");
    let staging = dir.path().join("staging");
    let staged = build_staged_release(&staging, "2.1.0");

    let error = apply_staged_release(
        &staged,
        &dir.path().join("nope"),
        &dir.path().join("backup"),
    )
    .unwrap_err();
    assert_eq!(error.code(), "update_install_dir_missing");
}

// ---------- 编排：查询 → 下载 → 校验 → 拉起安装器 / ZIP 回退 ----------

/// 用当前构建出的桌面端主程序当更新包：它有真实 PE 结构与当前文件版本，且未签名
/// （因此编排测试走预览通道，正好覆盖 signed=false / preview=true 的路径）。
/// 版本取自 workspace 版本，避免发版后与构建产物版本失配（`tools/check-version.ps1` 保证
/// tauri.conf.json 与 workspace 版本一致）。
const RELEASE_VERSION: &str = env!("CARGO_PKG_VERSION");
const CURRENT_VERSION: &str = "1.9.9";

fn release_executable_bytes() -> Vec<u8> {
    let path = PathBuf::from(env!("CARGO_BIN_EXE_agentnotify-desktop"));
    fs::read(&path).unwrap_or_else(|error| panic!("必须能读取构建产物 {}：{error}", path.display()))
}

fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    format!("{:x}", Sha256::digest(bytes))
}

fn release_json_with_both_assets() -> String {
    serde_json::json!({
        "tag_name": format!("v{RELEASE_VERSION}"),
        "body": "测试发布",
        "draft": false,
        "prerelease": false,
        "assets": [
            {"name": format!("Agent-notify-Setup-v{RELEASE_VERSION}.exe"), "url": "https://test.invalid/setup.exe"},
            {"name": format!("Agent-notify-v{RELEASE_VERSION}.zip"), "url": "https://test.invalid/archive.zip"},
            {"name": "SHA256SUMS.txt", "url": "https://test.invalid/SHA256SUMS.txt"},
        ],
    })
    .to_string()
}

fn release_zip_bytes(executable: &[u8]) -> Vec<u8> {
    let mut writer = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let options =
        zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    writer
        .start_file("Agent-notify/VERSION", options)
        .expect("写入版本文件");
    writer
        .write_all(RELEASE_VERSION.as_bytes())
        .expect("写入版本内容");
    writer
        .start_file("Agent-notify/bin/agentnotify-desktop.exe", options)
        .expect("写入主程序");
    writer.write_all(executable).expect("写入主程序内容");
    writer.finish().expect("完成测试 ZIP").into_inner()
}

/// 假传输：按地址分发 Release JSON / 校验清单 / 安装器 / ZIP，测试不访问真实网络。
struct ScriptedTransport {
    release_json: String,
    installer: Vec<u8>,
    archive: Vec<u8>,
    sums: String,
    downloaded: Mutex<Vec<String>>,
}

impl ScriptedTransport {
    fn new(executable: &[u8], archive: Vec<u8>) -> Self {
        let sums = format!(
            "{}  Agent-notify-Setup-v{RELEASE_VERSION}.exe\n{}  Agent-notify-v{RELEASE_VERSION}.zip\n",
            sha256_hex(executable),
            sha256_hex(&archive)
        );
        Self {
            release_json: release_json_with_both_assets(),
            installer: executable.to_vec(),
            archive,
            sums,
            downloaded: Mutex::new(Vec::new()),
        }
    }

    fn downloaded(&self) -> Vec<String> {
        self.downloaded.lock().expect("下载记录锁").clone()
    }
}

#[async_trait::async_trait]
impl UpdateTransport for ScriptedTransport {
    async fn get(
        &self,
        url: &str,
        _accept: &str,
        _limit: u64,
    ) -> Result<HttpTextResponse, UpdateError> {
        if url.ends_with("/releases/latest") {
            return Ok(HttpTextResponse {
                final_url: String::new(),
                body: self.release_json.as_bytes().to_vec(),
            });
        }
        Err(UpdateError::new(
            "update_test_unexpected_get",
            format!("测试未预期元数据请求：{url}"),
        ))
    }

    async fn download(
        &self,
        url: &str,
        destination: &Path,
        _limit: u64,
    ) -> Result<(), UpdateError> {
        self.downloaded
            .lock()
            .expect("下载记录锁")
            .push(url.to_owned());
        let bytes = if url.ends_with("setup.exe") {
            self.installer.clone()
        } else if url.ends_with("archive.zip") {
            self.archive.clone()
        } else if url.ends_with("SHA256SUMS.txt") {
            self.sums.as_bytes().to_vec()
        } else {
            return Err(UpdateError::new(
                "update_test_unexpected_download",
                format!("测试未预期下载：{url}"),
            ));
        };
        fs::write(destination, bytes).map_err(|error| {
            UpdateError::new(
                "update_test_write_failed",
                format!("写入测试文件失败：{error}"),
            )
        })
    }
}

fn update_service(dir: &Path, transport: std::sync::Arc<ScriptedTransport>) -> UpdateService {
    UpdateService::new(
        dir.to_path_buf(),
        UpdateConfig {
            repository: "srafyhucl-cpu/agent-notify-releases".to_owned(),
            api_base_url: "https://test.invalid".to_owned(),
        },
        transport,
    )
}

#[tokio::test]
async fn install_latest_reports_up_to_date_without_downloading_anything() {
    let dir = test_dir("agentnotify-service-latest-");
    let transport = std::sync::Arc::new(ScriptedTransport::new(b"MZ-placeholder", Vec::new()));
    let service = update_service(dir.path(), transport.clone());

    let error = service
        .install_latest(
            RELEASE_VERSION,
            UpdateChannel::Stable,
            dir.path(),
            &FakeLauncher::default(),
            &FakeExitRequester::default(),
        )
        .await
        .unwrap_err();

    assert_eq!(error.code(), "update_up_to_date");
    assert!(
        transport.downloaded().is_empty(),
        "已是最新版本时不得下载任何产物"
    );
}

#[tokio::test]
async fn install_latest_verifies_and_launches_the_installer_on_the_preview_channel() {
    let dir = test_dir("agentnotify-service-installer-");
    let executable = release_executable_bytes();
    let transport = std::sync::Arc::new(ScriptedTransport::new(&executable, Vec::new()));
    let service = update_service(dir.path(), transport);
    let install_root = dir.path().join("install");
    fs::create_dir_all(&install_root).expect("创建安装目录");
    let launcher = FakeLauncher::default();
    let exit = FakeExitRequester::default();

    let report = service
        .install_latest(
            CURRENT_VERSION,
            UpdateChannel::Beta,
            &install_root,
            &launcher,
            &exit,
        )
        .await
        .expect("安装器路径必须成功");

    assert_eq!(report.version, RELEASE_VERSION);
    assert_eq!(report.mode, InstallMode::Installer);
    assert!(!report.signed, "未签名预览包必须如实标记 signed=false");
    assert!(report.preview, "Beta 通道必须标记 preview=true");
    assert!(
        report.message.contains("安装程序已启动"),
        "{}",
        report.message
    );
    assert_eq!(exit.requested(), 1, "安装器成功拉起后必须请求应用优雅退出");

    let requests = launcher.requests();
    assert_eq!(requests.len(), 1);
    let values: Vec<String> = requests[0]
        .args
        .iter()
        .map(|value| value.to_string_lossy().into_owned())
        .collect();
    assert_eq!(values[0], "/SILENT");
    assert_eq!(values[1], "/NORESTART");
    assert!(values[2].starts_with("/LOG="));
    assert_eq!(
        values[3],
        format!("/DIR={}", install_root.display()),
        "升级必须锁定当前安装目录"
    );
}

#[tokio::test]
async fn install_latest_falls_back_to_the_zip_when_the_installer_fails() {
    let dir = test_dir("agentnotify-service-fallback-");
    let executable = release_executable_bytes();
    let archive = release_zip_bytes(&executable);
    let transport = std::sync::Arc::new(ScriptedTransport::new(&executable, archive));
    let service = update_service(dir.path(), transport);
    let install_root = dir.path().join("install");
    fs::create_dir_all(&install_root).expect("创建安装目录");
    fs::write(install_root.join("agentnotify-desktop.exe"), b"old-binary").expect("写入旧程序");
    let launcher = FakeLauncher::with_outcome(InstallerLaunchOutcome::Exited { code: Some(1) });
    let exit = FakeExitRequester::default();

    let report = service
        .install_latest(
            CURRENT_VERSION,
            UpdateChannel::Beta,
            &install_root,
            &launcher,
            &exit,
        )
        .await
        .expect("安装器失败必须回退 ZIP 替换");

    assert_eq!(report.mode, InstallMode::Archive);
    assert!(report.message.contains("离线包"), "{}", report.message);
    assert_eq!(
        exit.requested(),
        0,
        "安装器拉起失败时不得请求应用退出（回退替换后应用要继续运行）"
    );
    assert_eq!(
        fs::read(install_root.join("agentnotify-desktop.exe")).expect("新程序"),
        executable,
        "ZIP 回退必须把 bin/ 主程序替换到安装根目录"
    );

    let backup_root = dir.path().join("updates").join("backup");
    let backup_dirs: Vec<PathBuf> = fs::read_dir(&backup_root)
        .expect("备份目录必须存在")
        .map(|entry| entry.expect("备份条目").path())
        .collect();
    assert_eq!(backup_dirs.len(), 1, "必须留下可恢复的备份");
    assert_eq!(
        fs::read(backup_dirs[0].join("agentnotify-desktop.exe")).expect("备份程序"),
        b"old-binary"
    );
}

#[tokio::test]
async fn stable_archive_update_rejects_a_legacy_zip_before_touching_install_files() {
    let dir = test_dir("agentnotify-service-stable-manifest-");
    let executable = release_executable_bytes();
    let archive = release_zip_bytes(&executable);
    let mut scripted = ScriptedTransport::new(&executable, archive);
    scripted.release_json = serde_json::json!({
        "tag_name": format!("v{RELEASE_VERSION}"),
        "body": "只有 ZIP 的测试发布",
        "draft": false,
        "prerelease": false,
        "assets": [
            {"name": format!("Agent-notify-v{RELEASE_VERSION}.zip"), "url": "https://test.invalid/archive.zip"},
            {"name": "SHA256SUMS.txt", "url": "https://test.invalid/SHA256SUMS.txt"},
        ],
    })
    .to_string();
    let transport = std::sync::Arc::new(scripted);
    let service = update_service(dir.path(), transport);
    let install_root = dir.path().join("install");
    fs::create_dir_all(&install_root).expect("创建安装目录");
    fs::write(install_root.join("agentnotify-desktop.exe"), b"old-binary").expect("写入旧程序");

    let error = service
        .install_latest(
            CURRENT_VERSION,
            UpdateChannel::Stable,
            &install_root,
            &FakeLauncher::default(),
            &FakeExitRequester::default(),
        )
        .await
        .expect_err("Stable 不得安装没有发布清单的 ZIP");

    assert_eq!(error.code(), "update_manifest_missing");
    assert_eq!(
        fs::read(install_root.join("agentnotify-desktop.exe")).expect("旧程序"),
        b"old-binary",
        "清单失败时不得替换安装目录"
    );
}

/// 只有安装器成功拉起时才请求应用退出；拉起失败必须留在前台并把原因返回界面。
#[tokio::test]
async fn install_latest_requests_app_exit_only_after_a_successful_installer_launch() {
    let executable = release_executable_bytes();

    // 1. 安装器成功拉起：请求退出一次（由退出端口负责稍后优雅退出，不阻塞命令响应）。
    let launched_dir = test_dir("agentnotify-service-exit-launched-");
    let transport = std::sync::Arc::new(ScriptedTransport::new(&executable, Vec::new()));
    let service = update_service(launched_dir.path(), transport);
    let install_root = launched_dir.path().join("install");
    fs::create_dir_all(&install_root).expect("创建安装目录");
    let exit = FakeExitRequester::default();

    service
        .install_latest(
            CURRENT_VERSION,
            UpdateChannel::Beta,
            &install_root,
            &FakeLauncher::default(),
            &exit,
        )
        .await
        .expect("安装器路径必须成功");

    assert_eq!(
        exit.requested(),
        1,
        "安装器成功拉起后必须请求应用退出，安装器才能替换被占用的程序文件"
    );

    // 2. 安装器启动失败且 Release 里没有可回退的 ZIP：不请求退出，错误必须带原因返回。
    let failing_dir = test_dir("agentnotify-service-exit-failed-");
    let mut failing_transport = ScriptedTransport::new(&executable, Vec::new());
    failing_transport.release_json = serde_json::json!({
        "tag_name": format!("v{RELEASE_VERSION}"),
        "body": "只有安装器的测试发布",
        "draft": false,
        "prerelease": false,
        "assets": [
            {"name": format!("Agent-notify-Setup-v{RELEASE_VERSION}.exe"), "url": "https://test.invalid/setup.exe"},
            {"name": "SHA256SUMS.txt", "url": "https://test.invalid/SHA256SUMS.txt"},
        ],
    })
    .to_string();
    let transport = std::sync::Arc::new(failing_transport);
    let service = update_service(failing_dir.path(), transport);
    let install_root = failing_dir.path().join("install");
    fs::create_dir_all(&install_root).expect("创建安装目录");
    let exit = FakeExitRequester::default();
    let launcher = FakeLauncher::failing(UpdateError::new(
        "update_installer_failed",
        "无法启动更新安装器：拒绝访问",
    ));

    let error = service
        .install_latest(
            CURRENT_VERSION,
            UpdateChannel::Beta,
            &install_root,
            &launcher,
            &exit,
        )
        .await
        .unwrap_err();

    assert_eq!(
        exit.requested(),
        0,
        "安装器拉起失败时不得请求应用退出：应用要继续运行并把失败原因显示给用户"
    );
    assert!(
        error.message().contains("无法启动更新安装器"),
        "{}",
        error.message()
    );
}
