use std::{
    path::{Path, PathBuf},
    time::Duration,
};

use async_trait::async_trait;
use tokio::io::AsyncWriteExt;

use super::error::UpdateError;
use super::release::CHECKSUM_ASSET_NAME;
use super::verify::sha256_file;

/// 单个更新包（安装器或 ZIP）的大小上限。
pub const MAX_ARCHIVE_BYTES: u64 = 100 * 1024 * 1024;
/// SHA256SUMS.txt 与 Release JSON 等元数据的大小上限。
pub const MAX_CHECKSUMS_BYTES: u64 = 1024 * 1024;
/// 单次下载的总尝试次数：GitHub Release 资源偶发停滞，重试能显著降低“下载到一半失败”。
pub const DOWNLOAD_ATTEMPTS: u32 = 3;

const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(10 * 60);
const METADATA_TIMEOUT: Duration = Duration::from_secs(30);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
const ERROR_BODY_LIMIT: u64 = 4 * 1024;
const USER_AGENT: &str = "Agent-notify-updater/windows";

/// 一次文本响应：正文与重定向后的最终地址（GitHub Releases 的 HTML 回退需要最终地址）。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HttpTextResponse {
    pub final_url: String,
    pub body: Vec<u8>,
}

/// 更新流程的 HTTP 传输边界；运行时用 reqwest，测试注入假传输，不访问真实网络。
#[async_trait]
pub trait UpdateTransport: Send + Sync {
    /// 读取一个受限大小的响应正文（Release JSON / 校验清单 / HTML 回退页）。
    async fn get(
        &self,
        url: &str,
        accept: &str,
        limit: u64,
    ) -> Result<HttpTextResponse, UpdateError>;

    /// 流式下载到 destination，超过 limit 字节立即失败。
    async fn download(&self, url: &str, destination: &Path, limit: u64) -> Result<(), UpdateError>;
}

pub struct ReqwestUpdateTransport {
    client: reqwest::Client,
    token: Option<String>,
}

impl ReqwestUpdateTransport {
    pub fn new(token: Option<String>) -> Result<Self, UpdateError> {
        let client = reqwest::Client::builder()
            .connect_timeout(CONNECT_TIMEOUT)
            .timeout(DOWNLOAD_TIMEOUT)
            .build()
            .map_err(|error| {
                UpdateError::new(
                    "update_http_client_failed",
                    format!("创建更新 HTTP 客户端失败：{error}"),
                )
            })?;
        Ok(Self {
            client,
            token: token
                .map(|value| value.trim().to_owned())
                .filter(|value| !value.is_empty()),
        })
    }

    fn request(&self, url: &str, accept: &str) -> reqwest::RequestBuilder {
        let request = self
            .client
            .get(url)
            .header("User-Agent", USER_AGENT)
            .header("Accept", accept);
        match &self.token {
            Some(token) => request.header("Authorization", format!("Bearer {token}")),
            None => request,
        }
    }
}

#[async_trait]
impl UpdateTransport for ReqwestUpdateTransport {
    async fn get(
        &self,
        url: &str,
        accept: &str,
        limit: u64,
    ) -> Result<HttpTextResponse, UpdateError> {
        validate_url(url)?;
        let response = self
            .request(url, accept)
            .timeout(METADATA_TIMEOUT)
            .send()
            .await
            .map_err(map_reqwest_error)?;
        let final_url = response.url().to_string();
        let status = response.status();
        if !status.is_success() {
            return Err(response_error(status, response).await);
        }
        let body = response.bytes().await.map_err(map_reqwest_error)?;
        if body.len() as u64 > limit {
            return Err(response_too_large(limit));
        }
        Ok(HttpTextResponse {
            final_url,
            body: body.to_vec(),
        })
    }

    async fn download(&self, url: &str, destination: &Path, limit: u64) -> Result<(), UpdateError> {
        validate_url(url)?;
        let mut response = self
            .request(url, "application/octet-stream")
            .send()
            .await
            .map_err(map_reqwest_error)?;
        let status = response.status();
        if !status.is_success() {
            return Err(response_error(status, response).await);
        }

        let mut file = tokio::fs::File::create(destination)
            .await
            .map_err(|error| {
                UpdateError::new("update_io_failed", format!("写入更新文件失败：{error}"))
            })?;
        let mut written: u64 = 0;
        loop {
            let chunk = response.chunk().await.map_err(map_reqwest_error)?;
            let Some(chunk) = chunk else {
                break;
            };
            written += chunk.len() as u64;
            ensure_within_download_limit(written, limit)?;
            file.write_all(&chunk).await.map_err(|error| {
                UpdateError::new("update_io_failed", format!("写入更新文件失败：{error}"))
            })?;
        }
        file.flush().await.map_err(|error| {
            UpdateError::new("update_io_failed", format!("写入更新文件失败：{error}"))
        })?;
        Ok(())
    }
}

/// 下载过程中的大小上限检查：超过上限立即失败，不留半截文件。
pub fn ensure_within_download_limit(written: u64, limit: u64) -> Result<(), UpdateError> {
    if written > limit {
        return Err(UpdateError::new(
            "update_too_large",
            format!("更新文件超过允许大小 {limit} 字节"),
        ));
    }
    Ok(())
}

/// 带重试的下载：先写 `.part`，成功后再改名为目标文件；失败只清理临时文件。
pub async fn download_with_retry(
    transport: &dyn UpdateTransport,
    url: &str,
    destination: &Path,
    limit: u64,
    attempts: u32,
) -> Result<(), UpdateError> {
    let part = part_path(destination);
    let mut last_error = None;
    for _ in 0..attempts.max(1) {
        match transport.download(url, &part, limit).await {
            Ok(()) => {
                if let Err(error) = std::fs::rename(&part, destination) {
                    let _ = std::fs::remove_file(&part);
                    return Err(UpdateError::new(
                        "update_io_failed",
                        format!("保存更新文件失败：{error}"),
                    ));
                }
                return Ok(());
            }
            Err(error) => {
                let _ = std::fs::remove_file(&part);
                last_error = Some(error);
            }
        }
    }
    Err(last_error.unwrap_or_else(|| {
        UpdateError::new("update_download_failed", "下载更新文件失败，请稍后重试。")
    }))
}

/// 从 SHA256SUMS.txt 里取出指定产物的 SHA-256（与 Go 版解析规则一致）。
pub fn parse_checksum(checksums: &str, artifact_name: &str) -> Result<String, UpdateError> {
    for line in checksums.lines() {
        let fields: Vec<&str> = line.split_whitespace().collect();
        if fields.len() < 2 {
            continue;
        }
        let name = fields[fields.len() - 1].trim_start_matches('*');
        if name != artifact_name {
            continue;
        }
        let checksum = fields[0].trim().to_ascii_lowercase();
        if checksum.len() != 64 {
            return Err(UpdateError::new(
                "update_checksum_invalid",
                format!("更新包校验值长度无效：{checksum:?}"),
            ));
        }
        if !checksum.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err(UpdateError::new(
                "update_checksum_invalid",
                format!("更新包校验值格式无效：{checksum:?}"),
            ));
        }
        return Ok(checksum);
    }
    Err(UpdateError::new(
        "update_checksum_missing",
        format!("{CHECKSUM_ASSET_NAME} 未包含 {artifact_name}"),
    ))
}

/// 读取 SHA256SUMS.txt 并取出指定产物的期望校验值。
pub fn expected_checksum(
    checksums_path: &Path,
    artifact_name: &str,
) -> Result<String, UpdateError> {
    let contents = std::fs::read_to_string(checksums_path).map_err(|error| {
        UpdateError::new(
            "update_checksums_unreadable",
            format!("读取 {CHECKSUM_ASSET_NAME} 失败：{error}"),
        )
    })?;
    parse_checksum(&contents, artifact_name)
}

/// 比对实际 SHA-256 与发布清单（大小写不敏感），不一致时返回可展示的错误。
pub fn ensure_checksum_matches(expected: &str, actual: &str) -> Result<(), UpdateError> {
    if expected.eq_ignore_ascii_case(actual) {
        return Ok(());
    }
    Err(UpdateError::new(
        "update_checksum_mismatch",
        format!("更新包 SHA256 校验失败：期望 {expected}，实际 {actual}"),
    ))
}

/// 计算文件 SHA-256 并套用 update 错误类型。
pub fn hash_file(path: &Path) -> Result<String, UpdateError> {
    sha256_file(path).map_err(UpdateError::from)
}

fn part_path(destination: &Path) -> PathBuf {
    let mut value = destination.as_os_str().to_os_string();
    value.push(".part");
    PathBuf::from(value)
}

fn validate_url(raw_url: &str) -> Result<(), UpdateError> {
    let parsed = reqwest::Url::parse(raw_url).map_err(|_| {
        UpdateError::new("update_url_invalid", format!("更新地址无效：{raw_url:?}"))
    })?;
    if parsed.scheme().is_empty() || parsed.host_str().unwrap_or_default().is_empty() {
        return Err(UpdateError::new(
            "update_url_invalid",
            format!("更新地址无效：{raw_url:?}"),
        ));
    }
    Ok(())
}

fn response_too_large(limit: u64) -> UpdateError {
    UpdateError::new("update_too_large", format!("响应超过允许大小 {limit} 字节"))
}

async fn response_error(status: reqwest::StatusCode, response: reqwest::Response) -> UpdateError {
    if status == reqwest::StatusCode::NOT_FOUND {
        return UpdateError::new(
            "update_release_not_found",
            "GitHub Release 不存在或仓库未公开",
        );
    }
    let detail = match response.bytes().await {
        Ok(body) => String::from_utf8_lossy(&body[..body.len().min(ERROR_BODY_LIMIT as usize)])
            .trim()
            .to_owned(),
        Err(_) => String::new(),
    };
    if detail.is_empty() {
        UpdateError::new("update_http_failed", format!("HTTP {}", status.as_u16()))
    } else {
        UpdateError::new(
            "update_http_failed",
            format!("HTTP {}：{detail}", status.as_u16()),
        )
    }
}

fn map_reqwest_error(error: reqwest::Error) -> UpdateError {
    if error.is_timeout() {
        UpdateError::new(
            "update_timeout",
            "更新请求超时，请检查网络后重试。".to_owned(),
        )
    } else if error.is_connect() {
        UpdateError::new(
            "update_network_failed",
            "无法连接更新服务器，请检查网络后重试。".to_owned(),
        )
    } else if error.is_request() {
        UpdateError::new("update_request_failed", format!("更新请求失败：{error}"))
    } else {
        UpdateError::new(
            "update_network_failed",
            format!("更新网络请求失败：{error}"),
        )
    }
}
