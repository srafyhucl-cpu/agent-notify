use std::{
    collections::HashSet,
    ffi::OsString,
    fs,
    io::{self, Read, Write},
    path::{Component, Path, PathBuf},
    time::{Duration, SystemTime},
};

use async_trait::async_trait;

use super::{
    error::UpdateError,
    manifest::{VerifiedReleaseManifest, verify_release_manifest_at},
    release::normalize_version,
    verify::SignatureRequirement,
};

/// 解压后的更新内容大小上限（与 Go 版一致）。
pub const MAX_EXTRACTED_BYTES: u64 = 200 * 1024 * 1024;
/// 安装器立即退出的观察窗口：正常静默安装会持续数秒以上，
/// 窗口内非零退出说明参数、权限或安装包有问题，必须立刻告诉用户。
pub const INSTALLER_EARLY_EXIT_WINDOW: Duration = Duration::from_secs(2);
const INSTALLER_EARLY_EXIT_POLL: Duration = Duration::from_millis(100);

/// ZIP 内约定的发布根目录（与 tools/build-release.ps1 产物一致）。
pub const RELEASE_ROOT_NAME: &str = "Agent-notify";
/// ZIP 内主程序相对发布根目录的路径。
pub const RELEASE_EXECUTABLE_PATH: &str = "bin/agentnotify-desktop.exe";
/// ZIP 内版本文件相对发布根目录的路径。
pub const RELEASE_VERSION_PATH: &str = "VERSION";

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InstallerLaunchRequest {
    pub program: PathBuf,
    pub args: Vec<OsString>,
    pub working_dir: PathBuf,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InstallerLaunchOutcome {
    /// 观察窗口内仍在运行：静默安装已正常开始。
    StillRunning,
    /// 观察窗口内已退出；退出码为 0 视为安装器瞬间完成，仍是成功。
    Exited { code: Option<i32> },
}

/// 安装器启动端口：测试注入假启动器，不真的运行安装程序。
#[async_trait]
pub trait InstallerLauncher: Send + Sync {
    async fn launch(
        &self,
        request: InstallerLaunchRequest,
    ) -> Result<InstallerLaunchOutcome, UpdateError>;
}

/// 应用退出端口：安装器成功拉起后，请求应用自行优雅退出。
///
/// 实现必须**立即返回**：真正的退出放到后台任务里稍后执行，这样
/// 命令的响应能先回到界面（显示"正在安装"），安装器也不必久等应用释放文件。
/// 拉起安装器失败时不允许调用本端口，应用必须继续运行并把原因返回界面。
pub trait AppExitRequester: Send + Sync {
    fn request_exit(&self);
}

pub struct SystemInstallerLauncher;

#[async_trait]
impl InstallerLauncher for SystemInstallerLauncher {
    async fn launch(
        &self,
        request: InstallerLaunchRequest,
    ) -> Result<InstallerLaunchOutcome, UpdateError> {
        let mut command = tokio::process::Command::new(&request.program);
        command
            .args(&request.args)
            .current_dir(&request.working_dir);
        let mut child = command.spawn().map_err(|error| {
            UpdateError::new(
                "update_installer_failed",
                format!("无法启动更新安装器：{error}"),
            )
        })?;

        let started = tokio::time::Instant::now();
        loop {
            match child.try_wait() {
                Ok(Some(status)) => {
                    return Ok(InstallerLaunchOutcome::Exited {
                        code: status.code(),
                    });
                }
                Ok(None) => {}
                Err(error) => {
                    return Err(UpdateError::new(
                        "update_installer_failed",
                        format!("等待更新安装器失败：{error}"),
                    ));
                }
            }
            if started.elapsed() >= INSTALLER_EARLY_EXIT_WINDOW {
                // 安装器独立运行；本进程退出后它继续执行。
                return Ok(InstallerLaunchOutcome::StillRunning);
            }
            tokio::time::sleep(INSTALLER_EARLY_EXIT_POLL).await;
        }
    }
}

/// 安装器参数与 Go 版 `installerCommand` 一致：`/SILENT /NORESTART /LOG=<path>` + 附加参数。
///
/// 刻意**不**加 `/SUPPRESSMSGBOXES`：Inno 的内建提示（例如 Restart Manager 的「无法关闭应用」）
/// 在静默安装下仍会显示，用户点一次就能继续；若加上该开关，这类提示会变成静默中止安装，
/// 而此时应用已退出、也不会自动重启，反而更难恢复。安装器脚本里的自定义提示已按
/// `WizardSilent` 显式分支处理，不依赖这个开关。
pub fn installer_arguments(log_path: &Path, extra_args: &[OsString]) -> Vec<OsString> {
    let mut args = vec![
        OsString::from("/SILENT"),
        OsString::from("/NORESTART"),
        OsString::from(format!("/LOG={}", log_path.display())),
    ];
    args.extend(extra_args.iter().cloned());
    args
}

/// 拉起安装器；启动失败或观察窗口内非零退出都返回可展示的错误。
pub async fn launch_installer(
    installer: &Path,
    log_path: &Path,
    extra_args: &[OsString],
    launcher: &dyn InstallerLauncher,
) -> Result<(), UpdateError> {
    if !installer.is_file() {
        return Err(UpdateError::new(
            "update_installer_missing",
            format!("更新安装器不可用：{}", installer.display()),
        ));
    }
    let log_dir = log_path.parent().ok_or_else(|| {
        UpdateError::new(
            "update_log_dir_failed",
            format!("更新日志路径无效：{}", log_path.display()),
        )
    })?;
    fs::create_dir_all(log_dir).map_err(|error| {
        UpdateError::new(
            "update_log_dir_failed",
            format!("创建更新日志目录失败：{error}"),
        )
    })?;
    let working_dir = installer
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .map(Path::to_path_buf)
        .ok_or_else(|| {
            UpdateError::new(
                "update_installer_missing",
                format!("更新安装器路径无效：{}", installer.display()),
            )
        })?;

    let request = InstallerLaunchRequest {
        program: installer.to_path_buf(),
        args: installer_arguments(log_path, extra_args),
        working_dir,
    };
    match launcher.launch(request).await? {
        InstallerLaunchOutcome::StillRunning => Ok(()),
        InstallerLaunchOutcome::Exited { code: None } => Ok(()),
        InstallerLaunchOutcome::Exited { code: Some(0) } => Ok(()),
        InstallerLaunchOutcome::Exited { code: Some(code) } => Err(UpdateError::new(
            "update_installer_failed",
            format!(
                "更新安装器启动失败：安装器立即退出（退出码 {code}）（安装日志：{}）",
                log_path.display()
            ),
        )),
    }
}

/// ZIP 解包后的发布目录。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StagedRelease {
    pub root: PathBuf,
    pub executable: PathBuf,
    pub version_file: PathBuf,
    /// 已经通过清单或旧格式布局验证的安装文件相对路径。
    pub files: Vec<PathBuf>,
}

/// 解包并校验 ZIP 更新包：路径越界、符号链接与解包总量超限一律拒绝。
pub fn extract_archive(archive_path: &Path, destination: &Path) -> Result<(), UpdateError> {
    let archive_file = fs::File::open(archive_path).map_err(|error| {
        UpdateError::new(
            "update_archive_open_failed",
            format!("打开更新包失败：{error}"),
        )
    })?;
    let mut archive = zip::ZipArchive::new(archive_file).map_err(|error| {
        UpdateError::new(
            "update_archive_open_failed",
            format!("打开更新包失败：{error}"),
        )
    })?;

    let mut extracted: u64 = 0;
    let mut seen_paths = HashSet::new();
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).map_err(|error| {
            UpdateError::new(
                "update_archive_read_failed",
                format!("读取更新包失败：{error}"),
            )
        })?;
        let entry_name = entry.name().to_owned();
        let relative = safe_entry_path(&entry_name)?;
        let canonical_name = relative.to_string_lossy().replace('\\', "/").to_lowercase();
        if !seen_paths.insert(canonical_name) {
            return Err(UpdateError::new(
                "update_archive_path_unsafe",
                format!("更新包包含重复路径：{entry_name}"),
            ));
        }
        let target = destination.join(&relative);
        if !path_inside(destination, &target) {
            return Err(unsafe_path_error(&entry_name));
        }
        if entry.is_dir() {
            fs::create_dir_all(&target).map_err(|error| {
                UpdateError::new(
                    "update_extract_failed",
                    format!("创建更新目录失败：{error}"),
                )
            })?;
            continue;
        }
        if entry.is_symlink() {
            return Err(UpdateError::new(
                "update_archive_symlink",
                format!("更新包包含不支持的符号链接：{entry_name}"),
            ));
        }
        if !within_extraction_budget(entry.size(), extracted, MAX_EXTRACTED_BYTES) {
            return Err(extraction_budget_error(MAX_EXTRACTED_BYTES - extracted));
        }
        let written = write_entry(&mut entry, &target, MAX_EXTRACTED_BYTES - extracted)?;
        extracted += written;
    }
    Ok(())
}

/// 归一化 ZIP 内路径并校验是否越界，与 Go 版 `extractZip` 的检查一致。
pub fn safe_entry_path(name: &str) -> Result<PathBuf, UpdateError> {
    let cleaned = clean_slash_path(&name.replace('\\', "/"));
    if cleaned.is_empty()
        || cleaned == "."
        || cleaned == ".."
        || cleaned.starts_with("../")
        || cleaned.starts_with('/')
    {
        return Err(unsafe_path_error(name));
    }
    let native = PathBuf::from(cleaned.replace('/', std::path::MAIN_SEPARATOR_STR));
    match native.components().next() {
        None
        | Some(Component::Prefix(_))
        | Some(Component::RootDir)
        | Some(Component::ParentDir) => Err(unsafe_path_error(name)),
        _ => Ok(native),
    }
}

/// 校验解包后的发布目录：根目录、主程序与版本文件都必须存在且版本一致。
pub fn validate_staged_release(
    staging_root: &Path,
    expected_version: &str,
) -> Result<StagedRelease, UpdateError> {
    let root = staging_root.join(RELEASE_ROOT_NAME);
    if !root.is_dir() {
        return Err(UpdateError::new(
            "update_release_layout_invalid",
            "更新包缺少 Agent-notify 根目录",
        ));
    }
    let executable = root.join(RELEASE_EXECUTABLE_PATH);
    let version_file = root.join(RELEASE_VERSION_PATH);
    for required in [&executable, &version_file] {
        if !required.is_file() {
            return Err(UpdateError::new(
                "update_release_layout_invalid",
                format!("更新包缺少文件：{}", required.display()),
            ));
        }
    }
    let raw = fs::read_to_string(&version_file).map_err(|error| {
        UpdateError::new(
            "update_release_version_unreadable",
            format!("读取更新包版本失败：{error}"),
        )
    })?;
    if normalize_version(&raw).as_deref() != Some(expected_version) {
        return Err(UpdateError::new(
            "update_release_version_mismatch",
            format!(
                "更新包版本不一致：期望 {expected_version}，实际 {:?}",
                raw.trim()
            ),
        ));
    }
    Ok(StagedRelease {
        root,
        executable,
        version_file,
        files: Vec::new(),
    })
}

/// 在基础布局检查之后验证发布清单，并把清单声明的文件列表保存到暂存结果。
/// Beta 只有在清单与签名同时缺失时才保留旧文件集合；正式通道始终要求签名清单。
pub fn validate_staged_release_with_manifest(
    staging_root: &Path,
    expected_version: &str,
    signature_requirement: SignatureRequirement,
    now: SystemTime,
    trusted_thumbprints: &[String],
) -> Result<StagedRelease, UpdateError> {
    let mut staged = validate_staged_release(staging_root, expected_version)?;
    let files = match verify_release_manifest_at(
        &staged.root,
        expected_version,
        signature_requirement,
        now,
        trusted_thumbprints,
    )? {
        VerifiedReleaseManifest::Legacy => collect_files(&staged.root)?,
        VerifiedReleaseManifest::Signed { files } => files
            .into_iter()
            .map(|file| PathBuf::from(file.path))
            .collect(),
    };
    staged.files = files;
    Ok(staged)
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AppliedArchiveUpdate {
    pub replaced: Vec<PathBuf>,
    pub backup_dir: PathBuf,
}

fn is_safe_relative_file_path(path: &Path) -> bool {
    let mut components = path.components();
    let Some(Component::Normal(_)) = components.next() else {
        return false;
    };
    components.all(|component| matches!(component, Component::Normal(_)))
}

/// 把 ZIP 内的发布路径映射到安装目录：
/// - `bin/` 下的程序文件直接落到安装根目录（与安装器 `installer/agent-notify.iss` 一致）；
/// - `plugin/`、`tools/hooks/`、`VERSION` 等其余内容保持原有相对结构。
pub fn install_relative_path(release_relative: &Path) -> Option<PathBuf> {
    let mut components = release_relative.components();
    let first = components.next()?;
    if first
        .as_os_str()
        .to_string_lossy()
        .eq_ignore_ascii_case("bin")
    {
        let rest: PathBuf = components.collect();
        if rest.as_os_str().is_empty() {
            return None;
        }
        return Some(rest);
    }
    Some(release_relative.to_path_buf())
}

enum AppliedAction {
    Created(PathBuf),
    BackedUp(PathBuf),
}

/// 用解包好的发布目录替换安装目录里的程序文件。
/// 覆盖前先把旧文件改名到备份目录；任何一步失败都会回滚到替换前的状态。
pub fn apply_staged_release(
    staged: &StagedRelease,
    install_root: &Path,
    backup_dir: &Path,
) -> Result<AppliedArchiveUpdate, UpdateError> {
    if !install_root.is_dir() {
        return Err(UpdateError::new(
            "update_install_dir_missing",
            format!("安装目录不可用：{}", install_root.display()),
        ));
    }
    let files = &staged.files;
    if files.is_empty() {
        return Err(UpdateError::new(
            "update_release_layout_invalid",
            "更新包没有可安装的文件。",
        ));
    }
    fs::create_dir_all(backup_dir).map_err(|error| {
        UpdateError::new(
            "update_install_failed",
            format!("创建更新备份目录失败：{error}"),
        )
    })?;

    let mut actions: Vec<AppliedAction> = Vec::new();
    let mut replaced: Vec<PathBuf> = Vec::new();
    for release_relative in files {
        if !is_safe_relative_file_path(release_relative) {
            return Err(UpdateError::new(
                "update_release_layout_invalid",
                format!("更新包文件路径无效：{}", release_relative.display()),
            ));
        }
        let source = staged.root.join(release_relative);
        if !path_inside(&staged.root, &source) || !source.is_file() {
            return Err(UpdateError::new(
                "update_release_layout_invalid",
                format!("更新包缺少已验证文件：{}", release_relative.display()),
            ));
        }
        let Some(relative) = install_relative_path(release_relative) else {
            return Err(UpdateError::new(
                "update_release_layout_invalid",
                format!("更新包文件路径无效：{}", release_relative.display()),
            ));
        };
        let target = install_root.join(&relative);
        if !path_inside(install_root, &target) {
            return Err(UpdateError::new(
                "update_release_layout_invalid",
                format!("更新包文件路径越过安装目录：{}", release_relative.display()),
            ));
        }
        if let Err(error) = apply_file(&source, &target, backup_dir, &relative, &mut actions) {
            return Err(rollback_after_failure(
                error,
                &actions,
                install_root,
                backup_dir,
            ));
        }
        replaced.push(relative);
    }

    Ok(AppliedArchiveUpdate {
        replaced,
        backup_dir: backup_dir.to_path_buf(),
    })
}

fn apply_file(
    source: &Path,
    target: &Path,
    backup_dir: &Path,
    relative: &Path,
    actions: &mut Vec<AppliedAction>,
) -> Result<(), UpdateError> {
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent).map_err(|error| {
            UpdateError::new(
                "update_extract_failed",
                format!("创建更新目录失败：{error}"),
            )
        })?;
    }
    if target.is_file() {
        let backup = backup_dir.join(relative);
        if let Some(parent) = backup.parent() {
            fs::create_dir_all(parent).map_err(|error| {
                UpdateError::new(
                    "update_install_failed",
                    format!("创建更新备份目录失败：{error}"),
                )
            })?;
        }
        fs::rename(target, &backup).map_err(|error| {
            UpdateError::new(
                "update_install_failed",
                format!("备份旧文件失败（{}）：{error}", target.display()),
            )
        })?;
        actions.push(AppliedAction::BackedUp(relative.to_path_buf()));
    }
    fs::copy(source, target).map_err(|error| {
        UpdateError::new(
            "update_install_failed",
            format!("写入更新文件失败（{}）：{error}", target.display()),
        )
    })?;
    actions.push(AppliedAction::Created(relative.to_path_buf()));
    Ok(())
}

fn rollback_after_failure(
    error: UpdateError,
    actions: &[AppliedAction],
    install_root: &Path,
    backup_dir: &Path,
) -> UpdateError {
    match rollback(actions, install_root, backup_dir) {
        Ok(()) => error,
        Err(rollback_error) => UpdateError::new(
            "update_install_rollback_failed",
            format!(
                "{}；并且恢复旧文件失败：{}",
                error.message(),
                rollback_error.message()
            ),
        ),
    }
}

fn rollback(
    actions: &[AppliedAction],
    install_root: &Path,
    backup_dir: &Path,
) -> Result<(), UpdateError> {
    let mut first_error: Option<UpdateError> = None;
    for action in actions.iter().rev() {
        match action {
            AppliedAction::Created(relative) => {
                let target = install_root.join(relative);
                if let Err(error) = fs::remove_file(&target) {
                    if target.exists() && first_error.is_none() {
                        first_error = Some(UpdateError::new(
                            "update_install_rollback_failed",
                            format!("删除更新文件失败（{}）：{error}", target.display()),
                        ));
                    }
                }
            }
            AppliedAction::BackedUp(relative) => {
                let target = install_root.join(relative);
                let _ = fs::remove_file(&target);
                let backup = backup_dir.join(relative);
                if backup.exists() {
                    if let Err(error) = fs::rename(&backup, &target) {
                        if first_error.is_none() {
                            first_error = Some(UpdateError::new(
                                "update_install_rollback_failed",
                                format!("恢复旧文件失败（{}）：{error}", target.display()),
                            ));
                        }
                    }
                }
            }
        }
    }
    match first_error {
        Some(error) => Err(error),
        None => Ok(()),
    }
}

fn collect_files(root: &Path) -> Result<Vec<PathBuf>, UpdateError> {
    let mut files = Vec::new();
    collect_files_into(root, root, &mut files)?;
    files.sort();
    Ok(files)
}

fn collect_files_into(
    root: &Path,
    current: &Path,
    files: &mut Vec<PathBuf>,
) -> Result<(), UpdateError> {
    let entries = fs::read_dir(current).map_err(|error| {
        UpdateError::new(
            "update_install_failed",
            format!("读取更新文件失败（{}）：{error}", current.display()),
        )
    })?;
    for entry in entries {
        let entry = entry.map_err(|error| {
            UpdateError::new(
                "update_install_failed",
                format!("读取更新文件失败：{error}"),
            )
        })?;
        let path = entry.path();
        let file_type = entry.file_type().map_err(|error| {
            UpdateError::new(
                "update_install_failed",
                format!("读取更新文件失败：{error}"),
            )
        })?;
        if file_type.is_dir() {
            collect_files_into(root, &path, files)?;
        } else if file_type.is_file() {
            let relative = path.strip_prefix(root).map_err(|error| {
                UpdateError::new(
                    "update_install_failed",
                    format!("解析更新路径失败：{error}"),
                )
            })?;
            files.push(relative.to_path_buf());
        } else {
            return Err(UpdateError::new(
                "update_release_layout_invalid",
                "更新包包含不支持的链接或特殊文件。",
            ));
        }
    }
    Ok(())
}

fn write_entry(entry: &mut impl Read, target: &Path, limit: u64) -> Result<u64, UpdateError> {
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent).map_err(|error| {
            UpdateError::new(
                "update_extract_failed",
                format!("创建更新目录失败：{error}"),
            )
        })?;
    }
    let mut options = fs::OpenOptions::new();
    options.create(true).truncate(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        let mode = match target.extension().and_then(|value| value.to_str()) {
            Some(extension)
                if extension.eq_ignore_ascii_case("exe")
                    || extension.eq_ignore_ascii_case("ps1") =>
            {
                0o700
            }
            _ => 0o600,
        };
        options.mode(mode);
    }
    let mut file = options.open(target).map_err(|error| {
        UpdateError::new(
            "update_extract_failed",
            format!("写入更新文件失败：{error}"),
        )
    })?;
    let mut limited = entry.take(limit + 1);
    let written = io::copy(&mut limited, &mut file).map_err(|error| {
        UpdateError::new(
            "update_extract_failed",
            format!("解压更新文件失败：{error}"),
        )
    })?;
    if written > limit {
        return Err(extraction_budget_error(limit));
    }
    file.flush().map_err(|error| {
        UpdateError::new(
            "update_extract_failed",
            format!("写入更新文件失败：{error}"),
        )
    })?;
    Ok(written)
}

/// 全程使用 u64 累计，避免声明大小溢出为负数绕过检查（与 Go 版一致）。
pub fn within_extraction_budget(declared: u64, used: u64, max: u64) -> bool {
    if used > max {
        return false;
    }
    declared <= max - used
}

fn extraction_budget_error(remaining: u64) -> UpdateError {
    UpdateError::new(
        "update_archive_too_large",
        format!(
            "解压后的更新内容超过允许大小（本条剩余 {remaining} 字节，总计 {} 字节）",
            MAX_EXTRACTED_BYTES
        ),
    )
}

fn unsafe_path_error(name: &str) -> UpdateError {
    UpdateError::new(
        "update_archive_path_unsafe",
        format!("更新包包含越界路径：{name}"),
    )
}

/// Windows 路径大小写与分隔符都不敏感，做一次防御性包含判断
/// （真正的越界防护在 `safe_entry_path`，这里只是第二道保险）。
fn path_inside(root: &Path, target: &Path) -> bool {
    let root = root
        .to_string_lossy()
        .replace('/', "\\")
        .to_ascii_lowercase();
    let target = target
        .to_string_lossy()
        .replace('/', "\\")
        .to_ascii_lowercase();
    let root = root.trim_end_matches('\\');
    target == root || target.starts_with(&format!("{root}\\"))
}

/// 近似 Go 的 `path.Clean`：去掉 `.`/空段，按需弹出 `..`（根路径不允许越界）。
fn clean_slash_path(value: &str) -> String {
    let rooted = value.starts_with('/');
    let mut parts: Vec<&str> = Vec::new();
    for part in value.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                if matches!(parts.last(), Some(&"..")) || parts.is_empty() {
                    if !rooted {
                        parts.push("..");
                    }
                } else {
                    parts.pop();
                }
            }
            other => parts.push(other),
        }
    }
    let joined = parts.join("/");
    if rooted { format!("/{joined}") } else { joined }
}
