use std::cmp::Ordering;

use serde::Deserialize;

use super::download::{MAX_CHECKSUMS_BYTES, UpdateTransport};
use super::error::UpdateError;

/// 更新来源固定为镜像仓库；本地验证可用 AGENT_NOTIFY_UPDATE_REPOSITORY 覆盖（与 Go 版同名）。
pub const DEFAULT_REPOSITORY: &str = "srafyhucl-cpu/agent-notify-releases";
pub const REPOSITORY_ENV: &str = "AGENT_NOTIFY_UPDATE_REPOSITORY";
pub const DEFAULT_API_BASE_URL: &str = "https://api.github.com";
pub const API_BASE_ENV: &str = "AGENT_NOTIFY_UPDATE_API_BASE";
pub const DEFAULT_WEB_BASE_URL: &str = "https://github.com";
pub const CHECKSUM_ASSET_NAME: &str = "SHA256SUMS.txt";

const RELEASE_TAG_MARKER: &str = "/releases/tag/";

/// Release 中可用于升级的产物类型。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ArtifactKind {
    /// 正式安装器（Inno Setup），优先使用。
    Installer,
    /// 便携 ZIP，安装器缺失或启动失败时使用。
    Archive,
}

impl ArtifactKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Installer => "installer",
            Self::Archive => "archive",
        }
    }
}

/// 一个可安装的 Release。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReleaseInfo {
    pub version: String,
    pub tag_name: String,
    pub notes: String,
    pub artifact_kind: ArtifactKind,
    pub artifact_url: String,
    pub checksum_url: String,
    /// 选了安装器时，Release 里同时存在的 ZIP 地址；安装器启动失败时回退使用。
    pub archive_url: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
struct GithubRelease {
    #[serde(default)]
    tag_name: String,
    #[serde(default)]
    body: String,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    prerelease: bool,
    #[serde(default)]
    assets: Vec<GithubAsset>,
}

#[derive(Clone, Debug, Deserialize)]
struct GithubAsset {
    #[serde(default)]
    name: String,
    #[serde(default)]
    url: String,
    #[serde(default)]
    browser_download_url: String,
}

/// 查询最新 Release；本地版本不低于最新版本时返回 None。
pub async fn check_latest_release(
    transport: &dyn UpdateTransport,
    repository: &str,
    api_base_url: &str,
    current_version: &str,
) -> Result<Option<ReleaseInfo>, UpdateError> {
    if normalize_version(current_version).is_none() {
        return Err(UpdateError::new(
            "update_version_unsupported",
            format!("当前版本 {current_version:?} 不支持自动更新"),
        ));
    }
    let repository = repository.trim();
    validate_repository(repository)?;
    let api_base_url = api_base_url.trim().trim_end_matches('/');
    let api_base_url = if api_base_url.is_empty() {
        DEFAULT_API_BASE_URL
    } else {
        api_base_url
    };

    let request_url = format!(
        "{api_base_url}/repos/{}/releases/latest",
        escape_repository(repository)
    );
    let body = match transport
        .get(
            &request_url,
            "application/vnd.github+json",
            MAX_CHECKSUMS_BYTES,
        )
        .await
    {
        Ok(response) => response.body,
        Err(error) => {
            // GitHub API 被限流或不可用时退回 HTML 重定向，只支持 ZIP 产物（与 Go 版一致）。
            return match check_via_redirect(transport, repository, api_base_url, current_version)
                .await
            {
                Ok(result) => Ok(result),
                Err(_) => Err(UpdateError::new(
                    "update_check_failed",
                    format!("检查更新失败：{}", error.message()),
                )),
            };
        }
    };

    parse_latest_release(&body, current_version)
}

/// 解析 GitHub Release JSON；草稿/预发布视为不稳定，标签版本无效视为错误。
pub fn parse_latest_release(
    body: &[u8],
    current_version: &str,
) -> Result<Option<ReleaseInfo>, UpdateError> {
    let latest: GithubRelease = serde_json::from_slice(body).map_err(|error| {
        UpdateError::new(
            "update_release_parse_failed",
            format!("解析 Release 信息失败：{error}"),
        )
    })?;
    if latest.draft || latest.prerelease {
        return Err(UpdateError::new(
            "update_release_unstable",
            "最新 Release 不是稳定版本",
        ));
    }
    let version = normalize_version(&latest.tag_name).ok_or_else(|| {
        UpdateError::new(
            "update_release_tag_invalid",
            format!("Release 标签版本无效：{:?}", latest.tag_name),
        )
    })?;
    if !is_newer_version(&version, current_version)? {
        return Ok(None);
    }

    let installer_name = installer_asset_name(&version);
    let archive_name = archive_asset_name(&version);
    let (artifact_kind, artifact, archive_url) = match find_asset(&latest.assets, &installer_name) {
        Some(installer) => {
            let archive_url = find_asset(&latest.assets, &archive_name)
                .and_then(|archive| asset_download_url(&archive));
            (
                ArtifactKind::Installer,
                installer,
                archive_url.filter(|value| !value.is_empty()),
            )
        }
        None => match find_asset(&latest.assets, &archive_name) {
            Some(archive) => (ArtifactKind::Archive, archive, None),
            None => {
                return Err(UpdateError::new(
                    "update_asset_missing",
                    format!("Release 缺少更新包：{archive_name}"),
                ));
            }
        },
    };

    let checksums = find_asset(&latest.assets, CHECKSUM_ASSET_NAME).ok_or_else(|| {
        UpdateError::new(
            "update_checksum_asset_missing",
            format!("Release 缺少校验文件：{CHECKSUM_ASSET_NAME}"),
        )
    })?;
    let artifact_url = asset_download_url(&artifact).unwrap_or_default();
    let checksum_url = asset_download_url(&checksums).unwrap_or_default();
    if artifact_url.is_empty() || checksum_url.is_empty() {
        return Err(UpdateError::new(
            "update_download_url_missing",
            "Release 下载地址不完整",
        ));
    }

    Ok(Some(ReleaseInfo {
        version,
        tag_name: latest.tag_name.trim().to_owned(),
        notes: latest.body.trim().to_owned(),
        artifact_kind,
        artifact_url,
        checksum_url,
        archive_url,
    }))
}

/// HTML 回退：跟随 `/releases/latest` 重定向，从最终地址解析标签（只提供 ZIP 地址）。
async fn check_via_redirect(
    transport: &dyn UpdateTransport,
    repository: &str,
    api_base_url: &str,
    current_version: &str,
) -> Result<Option<ReleaseInfo>, UpdateError> {
    let web_base_url = if api_base_url != DEFAULT_API_BASE_URL {
        api_base_url
    } else {
        DEFAULT_WEB_BASE_URL
    };
    let request_url = format!(
        "{web_base_url}/{}/releases/latest",
        escape_repository(repository)
    );
    let response = transport
        .get(&request_url, "text/html", MAX_CHECKSUMS_BYTES)
        .await?;

    let tag_name = release_tag_from_url(&response.final_url)?;
    let version = normalize_version(&tag_name).ok_or_else(|| {
        UpdateError::new(
            "update_release_tag_invalid",
            format!("Release 标签版本无效：{tag_name:?}"),
        )
    })?;
    if !is_newer_version(&version, current_version)? {
        return Ok(None);
    }

    let download_base = format!(
        "{web_base_url}/{}/releases/download/{}/",
        escape_repository(repository),
        escape_path_segment(&tag_name)
    );
    let artifact_url = format!(
        "{download_base}{}",
        escape_path_segment(&archive_asset_name(&version))
    );
    let checksum_url = format!(
        "{download_base}{}",
        escape_path_segment(CHECKSUM_ASSET_NAME)
    );
    Ok(Some(ReleaseInfo {
        version,
        tag_name,
        notes: String::new(),
        artifact_kind: ArtifactKind::Archive,
        artifact_url,
        checksum_url,
        archive_url: None,
    }))
}

/// 从重定向后的地址里取出 Release 标签（URL 路径解码由调用方保证）。
pub fn release_tag_from_url(final_url: &str) -> Result<String, UpdateError> {
    let parsed = reqwest::Url::parse(final_url)
        .map_err(|_| UpdateError::new("update_release_url_invalid", "最新 Release 地址无法解析"))?;
    let segments: Vec<&str> = parsed
        .path_segments()
        .map(|segments| segments.collect())
        .unwrap_or_default();
    for window in segments.windows(3) {
        if window[0].eq_ignore_ascii_case("releases") && window[1].eq_ignore_ascii_case("tag") {
            let tag = window[2].trim();
            if !tag.is_empty() {
                return Ok(tag.to_owned());
            }
        }
    }
    // 兼容最终地址仍是 `/releases/tag/...` 字面量（未走到路径解析）的情况。
    if let Some(index) = final_url.find(RELEASE_TAG_MARKER) {
        let tag = final_url[index + RELEASE_TAG_MARKER.len()..]
            .split(['/', '?', '#'])
            .next()
            .unwrap_or_default()
            .trim();
        if !tag.is_empty() {
            return Ok(tag.to_owned());
        }
    }
    Err(UpdateError::new(
        "update_release_url_invalid",
        "最新 Release 地址无法解析",
    ))
}

pub fn installer_asset_name(version: &str) -> String {
    format!("Agent-notify-Setup-v{version}.exe")
}

pub fn archive_asset_name(version: &str) -> String {
    format!("Agent-notify-v{version}.zip")
}

/// 版本归一化（与 Go 版 `normalizeVersion` 一致）：去掉 v 前缀与 -/+ 后缀，必须是三段数字。
pub fn normalize_version(value: &str) -> Option<String> {
    let value = value.trim();
    let value = value.strip_prefix('v').unwrap_or(value);
    let value = value.split(['-', '+']).next().unwrap_or_default();
    if value.is_empty() {
        return None;
    }
    let parts: Vec<&str> = value.split('.').collect();
    if parts.len() != 3 {
        return None;
    }
    for part in &parts {
        if part.is_empty() || !part.bytes().all(|byte| byte.is_ascii_digit()) {
            return None;
        }
        part.parse::<u64>().ok()?;
    }
    Some(parts.join("."))
}

/// 三段数字版本比较；任一方无法归一化时返回明确错误。
pub fn compare_versions(candidate: &str, current: &str) -> Result<Ordering, UpdateError> {
    let Some(candidate_parts) = version_parts(candidate) else {
        return Err(not_comparable(candidate, current));
    };
    let Some(current_parts) = version_parts(current) else {
        return Err(not_comparable(candidate, current));
    };
    Ok(candidate_parts.cmp(&current_parts))
}

fn not_comparable(candidate: &str, current: &str) -> UpdateError {
    UpdateError::new(
        "update_version_not_comparable",
        format!("版本号无法比较：latest={candidate:?} current={current:?}"),
    )
}

pub fn is_newer_version(candidate: &str, current: &str) -> Result<bool, UpdateError> {
    Ok(compare_versions(candidate, current)? == Ordering::Greater)
}

fn version_parts(value: &str) -> Option<[u64; 3]> {
    let normalized = normalize_version(value)?;
    let mut parts = normalized.split('.');
    Some([
        parts.next()?.parse().ok()?,
        parts.next()?.parse().ok()?,
        parts.next()?.parse().ok()?,
    ])
}

fn find_asset(assets: &[GithubAsset], name: &str) -> Option<GithubAsset> {
    assets
        .iter()
        .find(|asset| asset.name.trim().eq_ignore_ascii_case(name))
        .cloned()
}

/// 与 Go 版一致：优先 API 资源地址，其次浏览器下载地址。
fn asset_download_url(asset: &GithubAsset) -> Option<String> {
    let url = asset.url.trim();
    if !url.is_empty() {
        return Some(url.to_owned());
    }
    let browser = asset.browser_download_url.trim();
    if !browser.is_empty() {
        return Some(browser.to_owned());
    }
    None
}

fn validate_repository(repository: &str) -> Result<(), UpdateError> {
    let parts: Vec<&str> = repository.split('/').collect();
    if parts.len() != 2 || parts[0].trim().is_empty() || parts[1].trim().is_empty() {
        return Err(UpdateError::new(
            "update_repository_invalid",
            format!("GitHub 仓库格式无效：{repository:?}"),
        ));
    }
    Ok(())
}

fn escape_repository(repository: &str) -> String {
    let parts: Vec<&str> = repository.split('/').collect();
    format!(
        "{}/{}",
        escape_path_segment(parts[0].trim()),
        escape_path_segment(parts[1].trim())
    )
}

fn escape_path_segment(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                escaped.push(byte as char)
            }
            other => escaped.push_str(&format!("%{other:02X}")),
        }
    }
    escaped
}
