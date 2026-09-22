use std::{
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use tokio::sync::Mutex;

use super::{
    download::{
        DOWNLOAD_ATTEMPTS, MAX_ARCHIVE_BYTES, MAX_CHECKSUMS_BYTES, ReqwestUpdateTransport,
        UpdateTransport, download_with_retry, ensure_checksum_matches, expected_checksum,
        hash_file,
    },
    error::UpdateError,
    install::{
        AppliedArchiveUpdate, InstallerLauncher, StagedRelease, apply_staged_release,
        extract_archive, launch_installer, validate_staged_release,
    },
    release::{
        API_BASE_ENV, ArtifactKind, CHECKSUM_ASSET_NAME, DEFAULT_API_BASE_URL, DEFAULT_REPOSITORY,
        REPOSITORY_ENV, ReleaseInfo, archive_asset_name, check_latest_release,
        installer_asset_name, is_newer_version,
    },
    verify::{SignatureRequirement, VerifiedUpdate, verify_download, verify_executable},
};

const UPDATE_DIR_NAME: &str = "updates";
const UPDATE_LOG_NAME: &str = "last-update.log";
const BACKUP_DIR_NAME: &str = "backup";
const EXTRACT_DIR_NAME: &str = "extracted";
/// 覆盖 GitHub Token 的环境变量（与 Go 版同名，公开仓库不需要）。
const GITHUB_TOKEN_ENV: &str = "AGENT_NOTIFY_GITHUB_TOKEN";

/// 更新通道：正式通道强制校验签名指纹，测试通道允许未签名包并标记 preview。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UpdateChannel {
    Stable,
    Beta,
}

impl UpdateChannel {
    pub fn is_preview(self) -> bool {
        matches!(self, Self::Beta)
    }

    fn signature_requirement(self) -> SignatureRequirement {
        if self.is_preview() {
            SignatureRequirement::Optional
        } else {
            SignatureRequirement::Required
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UpdateConfig {
    pub repository: String,
    pub api_base_url: String,
}

impl UpdateConfig {
    pub fn from_environment() -> Self {
        let repository = std::env::var(REPOSITORY_ENV)
            .ok()
            .map(|value| value.trim().to_owned())
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| DEFAULT_REPOSITORY.to_owned());
        let api_base_url = std::env::var(API_BASE_ENV)
            .ok()
            .map(|value| value.trim().trim_end_matches('/').to_owned())
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| DEFAULT_API_BASE_URL.to_owned());
        Self {
            repository,
            api_base_url,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InstallMode {
    Installer,
    Archive,
}

/// 一次成功安装的结果；`signed`/`preview` 必须如实带出，便于界面提示。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InstallReport {
    pub version: String,
    pub signed: bool,
    pub preview: bool,
    pub mode: InstallMode,
    pub message: String,
}

struct CachedRelease {
    preview: bool,
    release: ReleaseInfo,
}

/// 更新流程编排：查询最新 Release、下载校验、拉起安装器或回退 ZIP 替换。
pub struct UpdateService {
    transport: Arc<dyn UpdateTransport>,
    config: UpdateConfig,
    /// 更新包下载、解压与备份的根目录（`AppPaths.temp_dir` 下）。
    update_root: PathBuf,
    cached: Mutex<Option<CachedRelease>>,
    installing: AtomicBool,
}

impl UpdateService {
    /// 生产装配：reqwest 传输 + 环境变量配置。
    pub fn from_environment(paths: &crate::platform::AppPaths) -> Result<Self, UpdateError> {
        let token = std::env::var(GITHUB_TOKEN_ENV)
            .ok()
            .map(|value| value.trim().to_owned())
            .filter(|value| !value.is_empty());
        let transport = Arc::new(ReqwestUpdateTransport::new(token)?);
        Ok(Self::new(
            paths.temp_dir.clone(),
            UpdateConfig::from_environment(),
            transport,
        ))
    }

    pub fn new(
        temp_dir: PathBuf,
        config: UpdateConfig,
        transport: Arc<dyn UpdateTransport>,
    ) -> Self {
        Self {
            transport,
            config,
            update_root: temp_dir.join(UPDATE_DIR_NAME),
            cached: Mutex::new(None),
            installing: AtomicBool::new(false),
        }
    }

    pub fn config(&self) -> &UpdateConfig {
        &self.config
    }

    /// 查询最新 Release；本地版本不低于最新版本时返回 `None`。
    pub async fn check_latest(
        &self,
        current_version: &str,
        channel: UpdateChannel,
    ) -> Result<Option<ReleaseInfo>, UpdateError> {
        let release = check_latest_release(
            self.transport.as_ref(),
            &self.config.repository,
            &self.config.api_base_url,
            current_version,
        )
        .await?;
        *self.cached.lock().await = release.as_ref().map(|release| CachedRelease {
            preview: channel.is_preview(),
            release: release.clone(),
        });
        Ok(release)
    }

    /// 下载并校验最新版本，然后优先拉起安装器；安装器不可用时回退 ZIP 替换。
    pub async fn install_latest(
        &self,
        current_version: &str,
        channel: UpdateChannel,
        install_root: &Path,
        launcher: &dyn InstallerLauncher,
    ) -> Result<InstallReport, UpdateError> {
        let _guard = InstallingGuard::acquire(&self.installing)?;

        let release = match self.cached_release(channel).await {
            Some(release) => release,
            None => self
                .check_latest(current_version, channel)
                .await?
                .ok_or_else(up_to_date_error)?,
        };
        // 缓存可能来自更早的检查：安装前再比对一次，避免装上不比当前新的包。
        if !is_newer_version(&release.version, current_version)? {
            return Err(up_to_date_error());
        }

        let requirement = channel.signature_requirement();
        let prepared = self.prepare(&release, requirement).await?;
        self.install_prepared(&release, &prepared, install_root, launcher, requirement)
            .await
    }

    async fn cached_release(&self, channel: UpdateChannel) -> Option<ReleaseInfo> {
        self.cached
            .lock()
            .await
            .as_ref()
            .filter(|cached| cached.preview == channel.is_preview())
            .map(|cached| cached.release.clone())
    }

    async fn prepare(
        &self,
        release: &ReleaseInfo,
        requirement: SignatureRequirement,
    ) -> Result<PreparedUpdate, UpdateError> {
        let stage_dir = self.update_root.join(&release.version);
        if stage_dir.exists() {
            fs::remove_dir_all(&stage_dir).map_err(|error| {
                UpdateError::new(
                    "update_temp_dir_failed",
                    format!("清理旧更新目录失败：{error}"),
                )
            })?;
        }
        fs::create_dir_all(&stage_dir).map_err(|error| {
            UpdateError::new(
                "update_temp_dir_failed",
                format!("创建版本目录失败：{error}"),
            )
        })?;

        let checksums_path = stage_dir.join(CHECKSUM_ASSET_NAME);
        download_with_retry(
            self.transport.as_ref(),
            &release.checksum_url,
            &checksums_path,
            MAX_CHECKSUMS_BYTES,
            DOWNLOAD_ATTEMPTS,
        )
        .await
        .map_err(|error| {
            UpdateError::new(
                "update_checksums_download_failed",
                format!("下载校验文件失败：{}", error.message()),
            )
        })?;

        let artifact_name = match release.artifact_kind {
            ArtifactKind::Installer => installer_asset_name(&release.version),
            ArtifactKind::Archive => archive_asset_name(&release.version),
        };
        let artifact_path = stage_dir.join(&artifact_name);
        download_with_retry(
            self.transport.as_ref(),
            &release.artifact_url,
            &artifact_path,
            MAX_ARCHIVE_BYTES,
            DOWNLOAD_ATTEMPTS,
        )
        .await
        .map_err(map_artifact_download_error)?;

        let expected = expected_checksum(&checksums_path, &artifact_name)?;
        match release.artifact_kind {
            ArtifactKind::Installer => {
                let verified = verify_installer_artifact(
                    artifact_path.clone(),
                    expected,
                    requirement,
                    release.version.clone(),
                )
                .await?;
                Ok(PreparedUpdate {
                    version: release.version.clone(),
                    kind: ArtifactKind::Installer,
                    artifact_path,
                    checksums_path,
                    archive_url: release.archive_url.clone(),
                    verified,
                    staged_release: None,
                })
            }
            ArtifactKind::Archive => {
                let (staged, verified) = prepare_archive_artifact(
                    artifact_path.clone(),
                    stage_dir.clone(),
                    expected,
                    requirement,
                    release.version.clone(),
                )
                .await?;
                Ok(PreparedUpdate {
                    version: release.version.clone(),
                    kind: ArtifactKind::Archive,
                    artifact_path,
                    checksums_path,
                    archive_url: None,
                    verified,
                    staged_release: Some(staged),
                })
            }
        }
    }

    async fn install_prepared(
        &self,
        release: &ReleaseInfo,
        prepared: &PreparedUpdate,
        install_root: &Path,
        launcher: &dyn InstallerLauncher,
        requirement: SignatureRequirement,
    ) -> Result<InstallReport, UpdateError> {
        let preview = requirement == SignatureRequirement::Optional;
        if prepared.kind == ArtifactKind::Installer {
            let log_path = self.update_root.join(UPDATE_LOG_NAME);
            let extra_args = installer_extra_args(install_root);
            match launch_installer(&prepared.artifact_path, &log_path, &extra_args, launcher).await
            {
                Ok(()) => {
                    return Ok(InstallReport {
                        version: release.version.clone(),
                        signed: prepared.verified.signed,
                        preview,
                        mode: InstallMode::Installer,
                        message: format!(
                            "更新包已校验，安装程序已启动（v{}）。安装完成后 AgentNotify 会自动重新打开。",
                            release.version
                        ),
                    });
                }
                Err(installer_error) => {
                    let Some(archive_url) = prepared.archive_url.clone() else {
                        return Err(installer_error);
                    };
                    // 安装器失败：退回下载 ZIP 并替换安装目录文件；两条路径都失败时同时说明原因。
                    let signed = match self
                        .prepare_archive_fallback(prepared, &archive_url, install_root, requirement)
                        .await
                    {
                        Ok(signed) => signed,
                        Err(fallback_error) => {
                            return Err(UpdateError::new(
                                "update_install_failed",
                                format!(
                                    "更新安装器启动失败（{}），改用离线包替换也失败：{}",
                                    installer_error.message(),
                                    fallback_error.message()
                                ),
                            ));
                        }
                    };
                    return Ok(InstallReport {
                        version: release.version.clone(),
                        signed,
                        preview,
                        mode: InstallMode::Archive,
                        message: format!(
                            "安装程序未能启动（{}），已改用离线包替换：更新文件已就绪（v{}），重启 AgentNotify 后生效。",
                            installer_error.message(),
                            release.version
                        ),
                    });
                }
            }
        }

        let staged = prepared.staged_release.as_ref().ok_or_else(|| {
            UpdateError::new("update_release_layout_invalid", "更新包缺少已解压内容")
        })?;
        let backup_dir = self.backup_dir(&release.version);
        apply_archive(staged.clone(), install_root.to_path_buf(), backup_dir).await?;
        Ok(InstallReport {
            version: release.version.clone(),
            signed: prepared.verified.signed,
            preview,
            mode: InstallMode::Archive,
            message: format!(
                "更新文件已就绪（v{}），重启 AgentNotify 后生效。",
                release.version
            ),
        })
    }

    /// 安装器失败时的回退：下载 ZIP、校验 SHA256、解包并校验主程序签名，再替换安装目录。
    async fn prepare_archive_fallback(
        &self,
        prepared: &PreparedUpdate,
        archive_url: &str,
        install_root: &Path,
        requirement: SignatureRequirement,
    ) -> Result<bool, UpdateError> {
        let stage_dir = self.update_root.join(&prepared.version);
        let archive_name = archive_asset_name(&prepared.version);
        let archive_path = stage_dir.join(&archive_name);
        download_with_retry(
            self.transport.as_ref(),
            archive_url,
            &archive_path,
            MAX_ARCHIVE_BYTES,
            DOWNLOAD_ATTEMPTS,
        )
        .await
        .map_err(map_artifact_download_error)?;

        let expected = expected_checksum(&prepared.checksums_path, &archive_name)?;
        let (staged, verified) = prepare_archive_artifact(
            archive_path,
            stage_dir,
            expected,
            requirement,
            prepared.version.clone(),
        )
        .await?;
        let backup_dir = self.backup_dir(&prepared.version);
        apply_archive(staged, install_root.to_path_buf(), backup_dir).await?;
        Ok(verified.signed)
    }

    fn backup_dir(&self, version: &str) -> PathBuf {
        // 备份目录带随机后缀：旧备份可能包含仍被占用的旧版程序文件，不能复用同名目录。
        self.update_root
            .join(BACKUP_DIR_NAME)
            .join(format!("{version}-{}", uuid::Uuid::new_v4().simple()))
    }
}

struct PreparedUpdate {
    version: String,
    kind: ArtifactKind,
    artifact_path: PathBuf,
    checksums_path: PathBuf,
    archive_url: Option<String>,
    verified: VerifiedUpdate,
    staged_release: Option<StagedRelease>,
}

async fn verify_installer_artifact(
    artifact_path: PathBuf,
    expected_checksum: String,
    requirement: SignatureRequirement,
    version: String,
) -> Result<VerifiedUpdate, UpdateError> {
    tokio::task::spawn_blocking(move || {
        verify_download(
            &artifact_path,
            &expected_checksum,
            requirement,
            Some(&version),
        )
        .map_err(UpdateError::from)
    })
    .await
    .map_err(|error| UpdateError::new("update_task_failed", format!("校验更新包失败：{error}")))?
}

async fn prepare_archive_artifact(
    archive_path: PathBuf,
    stage_dir: PathBuf,
    expected_checksum: String,
    requirement: SignatureRequirement,
    version: String,
) -> Result<(StagedRelease, VerifiedUpdate), UpdateError> {
    tokio::task::spawn_blocking(move || {
        let actual = hash_file(&archive_path)?;
        ensure_checksum_matches(&expected_checksum, &actual)?;

        let extract_dir = stage_dir.join(EXTRACT_DIR_NAME);
        if extract_dir.exists() {
            fs::remove_dir_all(&extract_dir).map_err(|error| {
                UpdateError::new(
                    "update_temp_dir_failed",
                    format!("清理旧解压目录失败：{error}"),
                )
            })?;
        }
        fs::create_dir_all(&extract_dir).map_err(|error| {
            UpdateError::new(
                "update_temp_dir_failed",
                format!("创建解压目录失败：{error}"),
            )
        })?;
        extract_archive(&archive_path, &extract_dir)?;
        let staged = validate_staged_release(&extract_dir, &version)?;
        let verified = verify_executable(&staged.executable, requirement, Some(&version))?;
        Ok((staged, verified))
    })
    .await
    .map_err(|error| UpdateError::new("update_task_failed", format!("解压更新包失败：{error}")))?
}

async fn apply_archive(
    staged: StagedRelease,
    install_root: PathBuf,
    backup_dir: PathBuf,
) -> Result<AppliedArchiveUpdate, UpdateError> {
    tokio::task::spawn_blocking(move || apply_staged_release(&staged, &install_root, &backup_dir))
        .await
        .map_err(|error| {
            UpdateError::new("update_task_failed", format!("替换更新文件失败：{error}"))
        })?
}

fn installer_extra_args(install_root: &Path) -> Vec<OsString> {
    if install_root.as_os_str().is_empty() {
        return Vec::new();
    }
    vec![OsString::from(format!("/DIR={}", install_root.display()))]
}

fn map_artifact_download_error(error: UpdateError) -> UpdateError {
    if error.code() == "update_timeout" {
        return UpdateError::new(
            "update_download_timeout",
            format!(
                "下载更新包超时（网络较慢或被限速）：请稍后重试，或到 Releases 页面手动下载安装包。原始错误：{}",
                error.message()
            ),
        );
    }
    UpdateError::new(
        "update_download_failed",
        format!("下载更新包失败：{}", error.message()),
    )
}

fn up_to_date_error() -> UpdateError {
    UpdateError::new("update_up_to_date", "当前已是最新版本，无需安装。")
}

/// 同一时间只允许一个安装任务，避免重复点击并发下载/替换。
struct InstallingGuard<'a>(&'a AtomicBool);

impl<'a> InstallingGuard<'a> {
    fn acquire(flag: &'a AtomicBool) -> Result<Self, UpdateError> {
        if flag.swap(true, Ordering::AcqRel) {
            return Err(UpdateError::new(
                "update_install_busy",
                "已有更新任务正在进行，请等待完成。",
            ));
        }
        Ok(Self(flag))
    }
}

impl Drop for InstallingGuard<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}
