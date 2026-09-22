use std::{collections::VecDeque, path::Path, sync::Mutex};

use agentnotify_desktop::update::{
    ArtifactKind, HttpTextResponse, UpdateError, UpdateTransport, check_latest_release,
    compare_versions, is_newer_version, normalize_version, parse_latest_release,
    release_tag_from_url,
};

/// 假传输：按队列返回预置响应，记录请求地址，测试不访问真实网络。
#[derive(Default)]
struct FakeTransport {
    get_responses: Mutex<VecDeque<Result<HttpTextResponse, UpdateError>>>,
    get_urls: Mutex<Vec<String>>,
}

impl FakeTransport {
    fn with_responses(responses: Vec<Result<HttpTextResponse, UpdateError>>) -> Self {
        Self {
            get_responses: Mutex::new(responses.into()),
            get_urls: Mutex::new(Vec::new()),
        }
    }

    fn json(body: &str) -> Result<HttpTextResponse, UpdateError> {
        Ok(HttpTextResponse {
            final_url: String::new(),
            body: body.as_bytes().to_vec(),
        })
    }

    fn urls(&self) -> Vec<String> {
        self.get_urls.lock().expect("请求记录锁").clone()
    }
}

#[async_trait::async_trait]
impl UpdateTransport for FakeTransport {
    async fn get(
        &self,
        url: &str,
        _accept: &str,
        _limit: u64,
    ) -> Result<HttpTextResponse, UpdateError> {
        self.get_urls
            .lock()
            .expect("请求记录锁")
            .push(url.to_owned());
        self.get_responses
            .lock()
            .expect("响应队列锁")
            .pop_front()
            .unwrap_or_else(|| {
                Err(UpdateError::new(
                    "update_test_no_response",
                    "测试未提供更多响应",
                ))
            })
    }

    async fn download(
        &self,
        _url: &str,
        _destination: &Path,
        _limit: u64,
    ) -> Result<(), UpdateError> {
        Err(UpdateError::new(
            "update_test_unexpected_download",
            "Release 查询测试不应下载文件",
        ))
    }
}

fn release_json(tag: &str, asset_names: &[&str]) -> String {
    let assets: Vec<serde_json::Value> = asset_names
        .iter()
        .map(|name| {
            serde_json::json!({
                "name": name,
                "url": format!("https://api.github.com/assets/{name}"),
                "browser_download_url": format!("https://github.com/download/{name}"),
            })
        })
        .collect();
    serde_json::json!({
        "tag_name": tag,
        "body": "修复若干问题",
        "draft": false,
        "prerelease": false,
        "assets": assets,
    })
    .to_string()
}

#[test]
fn version_normalization_matches_go_rules() {
    assert_eq!(normalize_version("v2.0.0"), Some("2.0.0".to_owned()));
    assert_eq!(normalize_version(" 2.0.0 "), Some("2.0.0".to_owned()));
    assert_eq!(
        normalize_version("2.0.0-dev.0"),
        Some("2.0.0".to_owned()),
        "预发布后缀必须被截掉"
    );
    assert_eq!(normalize_version("2.0.0+build.7"), Some("2.0.0".to_owned()));
    assert_eq!(normalize_version("2.0"), None, "必须是三段版本");
    assert_eq!(normalize_version("2.0.0.1"), None, "必须是三段版本");
    assert_eq!(normalize_version(""), None);
    assert_eq!(normalize_version("latest"), None);
    assert_eq!(normalize_version("2.x.0"), None);
}

#[test]
fn version_comparison_is_numeric_and_reports_uncomparable_versions() {
    assert_eq!(
        compare_versions("2.0.1", "2.0.0").expect("可比较"),
        std::cmp::Ordering::Greater
    );
    assert_eq!(
        compare_versions("2.0.0", "2.0.0").expect("可比较"),
        std::cmp::Ordering::Equal
    );
    assert_eq!(
        compare_versions("1.9.9", "2.0.0").expect("可比较"),
        std::cmp::Ordering::Less
    );
    assert!(!is_newer_version("2.0.0", "2.0.0").expect("可比较"));
    assert!(is_newer_version("2.10.0", "2.9.9").expect("可比较"));

    let error = compare_versions("2.0", "2.0.0").unwrap_err();
    assert_eq!(error.code(), "update_version_not_comparable");
    assert!(error.message().contains("2.0"));
}

#[test]
fn release_parsing_prefers_installer_and_keeps_archive_fallback_url() {
    let body = release_json(
        "v2.1.0",
        &[
            "Agent-notify-Setup-v2.1.0.exe",
            "Agent-notify-v2.1.0.zip",
            "SHA256SUMS.txt",
        ],
    );
    let release = parse_latest_release(body.as_bytes(), "2.0.0")
        .expect("解析必须成功")
        .expect("必须发现新版本");

    assert_eq!(release.version, "2.1.0");
    assert_eq!(release.tag_name, "v2.1.0");
    assert_eq!(release.notes, "修复若干问题");
    assert_eq!(release.artifact_kind, ArtifactKind::Installer);
    assert_eq!(
        release.artifact_url,
        "https://api.github.com/assets/Agent-notify-Setup-v2.1.0.exe"
    );
    assert_eq!(
        release.checksum_url,
        "https://api.github.com/assets/SHA256SUMS.txt"
    );
    assert_eq!(
        release.archive_url.as_deref(),
        Some("https://api.github.com/assets/Agent-notify-v2.1.0.zip"),
        "安装器失败回退 ZIP 时必须能拿到 ZIP 地址"
    );
}

#[test]
fn release_parsing_falls_back_to_archive_and_uses_browser_download_url() {
    let body = release_json("v2.1.0", &["Agent-notify-v2.1.0.zip", "SHA256SUMS.txt"]);
    let release = parse_latest_release(body.as_bytes(), "2.0.0")
        .expect("解析必须成功")
        .expect("必须发现新版本");
    assert_eq!(release.artifact_kind, ArtifactKind::Archive);
    assert_eq!(release.archive_url, None);

    let browser_only = serde_json::json!({
        "tag_name": "v2.1.0",
        "assets": [
            {"name": "Agent-notify-v2.1.0.zip", "browser_download_url": "https://github.com/download/zip"},
            {"name": "SHA256SUMS.txt", "browser_download_url": "https://github.com/download/sums"},
        ],
    })
    .to_string();
    let release = parse_latest_release(browser_only.as_bytes(), "2.0.0")
        .expect("解析必须成功")
        .expect("必须发现新版本");
    assert_eq!(release.artifact_url, "https://github.com/download/zip");
    assert_eq!(release.checksum_url, "https://github.com/download/sums");
}

#[test]
fn release_parsing_rejects_missing_assets_with_explicit_reasons() {
    let no_packages = release_json("v2.1.0", &["SHA256SUMS.txt"]);
    let error = parse_latest_release(no_packages.as_bytes(), "2.0.0").unwrap_err();
    assert_eq!(error.code(), "update_asset_missing");
    assert!(error.message().contains("Agent-notify-v2.1.0.zip"));

    let no_checksums = release_json("v2.1.0", &["Agent-notify-Setup-v2.1.0.exe"]);
    let error = parse_latest_release(no_checksums.as_bytes(), "2.0.0").unwrap_err();
    assert_eq!(error.code(), "update_checksum_asset_missing");
    assert!(error.message().contains("SHA256SUMS.txt"));
}

#[test]
fn release_parsing_rejects_draft_prerelease_and_invalid_tags() {
    let draft = serde_json::json!({
        "tag_name": "v2.1.0",
        "draft": true,
        "prerelease": false,
        "assets": [],
    })
    .to_string();
    let error = parse_latest_release(draft.as_bytes(), "2.0.0").unwrap_err();
    assert_eq!(error.code(), "update_release_unstable");

    let prerelease = serde_json::json!({
        "tag_name": "v2.1.0",
        "draft": false,
        "prerelease": true,
        "assets": [],
    })
    .to_string();
    let error = parse_latest_release(prerelease.as_bytes(), "2.0.0").unwrap_err();
    assert_eq!(error.code(), "update_release_unstable");

    let invalid_tag = release_json("latest", &[]);
    let error = parse_latest_release(invalid_tag.as_bytes(), "2.0.0").unwrap_err();
    assert_eq!(error.code(), "update_release_tag_invalid");

    let broken_json = b"not json";
    let error = parse_latest_release(broken_json, "2.0.0").unwrap_err();
    assert_eq!(error.code(), "update_release_parse_failed");
}

#[test]
fn release_parsing_treats_equal_or_older_versions_as_up_to_date() {
    let equal = release_json("v2.0.0", &["Agent-notify-v2.0.0.zip", "SHA256SUMS.txt"]);
    assert_eq!(
        parse_latest_release(equal.as_bytes(), "2.0.0").expect("解析必须成功"),
        None
    );

    let older = release_json("v1.9.9", &["Agent-notify-v1.9.9.zip", "SHA256SUMS.txt"]);
    assert_eq!(
        parse_latest_release(older.as_bytes(), "2.0.0").expect("解析必须成功"),
        None,
        "本地版本更高时必须视为已是最新"
    );
}

#[tokio::test]
async fn check_latest_release_queries_the_fixed_repository() {
    let transport = FakeTransport::with_responses(vec![FakeTransport::json(&release_json(
        "v2.1.0",
        &[
            "Agent-notify-Setup-v2.1.0.exe",
            "Agent-notify-v2.1.0.zip",
            "SHA256SUMS.txt",
        ],
    ))]);

    let release = check_latest_release(
        &transport,
        "srafyhucl-cpu/agent-notify-releases",
        "https://api.github.com",
        "2.0.0",
    )
    .await
    .expect("查询必须成功")
    .expect("必须发现新版本");

    assert_eq!(release.version, "2.1.0");
    assert_eq!(
        transport.urls(),
        vec!["https://api.github.com/repos/srafyhucl-cpu/agent-notify-releases/releases/latest"]
    );
}

#[tokio::test]
async fn check_latest_release_falls_back_to_html_redirect_when_api_is_unavailable() {
    let transport = FakeTransport::with_responses(vec![
        Err(UpdateError::new(
            "update_http_failed",
            "HTTP 403：API rate limit exceeded",
        )),
        Ok(HttpTextResponse {
            final_url: "https://github.com/srafyhucl-cpu/agent-notify-releases/releases/tag/v2.1.0"
                .to_owned(),
            body: b"<html></html>".to_vec(),
        }),
    ]);

    let release = check_latest_release(
        &transport,
        "srafyhucl-cpu/agent-notify-releases",
        "https://api.github.com",
        "2.0.0",
    )
    .await
    .expect("HTML 回退必须成功")
    .expect("必须发现新版本");

    assert_eq!(release.version, "2.1.0");
    assert_eq!(release.artifact_kind, ArtifactKind::Archive);
    assert_eq!(
        release.artifact_url,
        "https://github.com/srafyhucl-cpu/agent-notify-releases/releases/download/v2.1.0/Agent-notify-v2.1.0.zip"
    );
    assert_eq!(
        release.checksum_url,
        "https://github.com/srafyhucl-cpu/agent-notify-releases/releases/download/v2.1.0/SHA256SUMS.txt"
    );
    assert_eq!(transport.urls().len(), 2, "必须先走 API，再回退 HTML");
}

#[tokio::test]
async fn check_latest_release_reports_the_api_error_when_both_paths_fail() {
    let transport = FakeTransport::with_responses(vec![
        Err(UpdateError::new("update_http_failed", "HTTP 500")),
        Err(UpdateError::new("update_network_failed", "无法连接")),
    ]);

    let error = check_latest_release(
        &transport,
        "srafyhucl-cpu/agent-notify-releases",
        "https://api.github.com",
        "2.0.0",
    )
    .await
    .unwrap_err();

    assert_eq!(error.code(), "update_check_failed");
    assert!(
        error.message().contains("HTTP 500"),
        "必须暴露真实失败原因而不是假装已是最新：{}",
        error.message()
    );
}

#[tokio::test]
async fn check_latest_release_rejects_unparsable_current_version_before_network() {
    let transport = FakeTransport::default();
    let error = check_latest_release(
        &transport,
        "srafyhucl-cpu/agent-notify-releases",
        "https://api.github.com",
        "2.0",
    )
    .await
    .unwrap_err();
    assert_eq!(error.code(), "update_version_unsupported");
    assert!(transport.urls().is_empty(), "版本不可比时不应发起网络请求");
}

#[tokio::test]
async fn check_latest_release_rejects_invalid_repository_format() {
    let transport = FakeTransport::default();
    let error = check_latest_release(&transport, "only-owner", "https://api.github.com", "2.0.0")
        .await
        .unwrap_err();
    assert_eq!(error.code(), "update_repository_invalid");
}

#[test]
fn release_tag_is_read_from_the_redirect_target() {
    assert_eq!(
        release_tag_from_url(
            "https://github.com/srafyhucl-cpu/agent-notify-releases/releases/tag/v2.1.0"
        )
        .expect("标签必须可解析"),
        "v2.1.0"
    );
    assert_eq!(
        release_tag_from_url(
            "https://github.com/srafyhucl-cpu/agent-notify-releases/releases/tag/v2.1.0?tab=readme"
        )
        .expect("查询串必须被忽略"),
        "v2.1.0"
    );
    let error = release_tag_from_url("https://github.com/owner/repo/releases").unwrap_err();
    assert_eq!(error.code(), "update_release_url_invalid");
}
