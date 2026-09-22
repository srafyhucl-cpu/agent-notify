use std::{
    collections::VecDeque,
    path::{Path, PathBuf},
    sync::Mutex,
};

use agentnotify_desktop::update::{
    DOWNLOAD_ATTEMPTS, HttpTextResponse, UpdateError, UpdateTransport, download_with_retry,
    ensure_checksum_matches, ensure_within_download_limit, expected_checksum, parse_checksum,
};

/// 假传输：按脚本返回结果，可选地把字节写进目标文件，测试不访问真实网络。
#[derive(Default)]
struct FakeTransport {
    downloads: Mutex<VecDeque<DownloadOutcome>>,
    attempts: Mutex<u32>,
}

enum DownloadOutcome {
    Write(Vec<u8>),
    /// 模拟下载中断：先留下半截内容，再返回失败。
    WriteThenFail(Vec<u8>, UpdateError),
    Fail(UpdateError),
}

impl FakeTransport {
    fn with_downloads(outcomes: Vec<DownloadOutcome>) -> Self {
        Self {
            downloads: Mutex::new(outcomes.into()),
            attempts: Mutex::new(0),
        }
    }

    fn attempts(&self) -> u32 {
        *self.attempts.lock().expect("尝试次数锁")
    }
}

#[async_trait::async_trait]
impl UpdateTransport for FakeTransport {
    async fn get(
        &self,
        _url: &str,
        _accept: &str,
        _limit: u64,
    ) -> Result<HttpTextResponse, UpdateError> {
        Err(UpdateError::new(
            "update_test_unexpected_get",
            "下载测试不应发起元数据请求",
        ))
    }

    async fn download(
        &self,
        _url: &str,
        destination: &Path,
        _limit: u64,
    ) -> Result<(), UpdateError> {
        *self.attempts.lock().expect("尝试次数锁") += 1;
        match self
            .downloads
            .lock()
            .expect("下载脚本锁")
            .pop_front()
            .unwrap_or_else(|| {
                DownloadOutcome::Fail(UpdateError::new(
                    "update_test_no_download",
                    "测试未提供更多下载结果",
                ))
            }) {
            DownloadOutcome::Write(bytes) => std::fs::write(destination, bytes).map_err(|error| {
                UpdateError::new(
                    "update_test_write_failed",
                    format!("写入测试文件失败：{error}"),
                )
            }),
            DownloadOutcome::WriteThenFail(bytes, error) => {
                std::fs::write(destination, bytes).map_err(|write_error| {
                    UpdateError::new(
                        "update_test_write_failed",
                        format!("写入测试文件失败：{write_error}"),
                    )
                })?;
                Err(error)
            }
            DownloadOutcome::Fail(error) => Err(error),
        }
    }
}

fn test_dir(prefix: &str) -> tempfile::TempDir {
    tempfile::Builder::new()
        .prefix(prefix)
        .tempdir_in(agentnotify_testkit::test_temp_root())
        .expect("测试临时目录必须可创建")
}

#[test]
fn sha256sums_parsing_matches_go_rules() {
    let sums = "\
aaaa1111bbbb2222cccc3333dddd4444eeee5555ffff6666aaaa7777bbbb8888  Agent-notify-Setup-v2.1.0.exe
ABCDEF0123456789ABCDEF0123456789ABCDEF0123456789ABCDEF0123456789 *Agent-notify-v2.1.0.zip
";
    assert_eq!(
        parse_checksum(sums, "Agent-notify-v2.1.0.zip").expect("必须找到校验值"),
        "abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789",
        "`*` 前缀与大小写必须被规范化"
    );

    let error = parse_checksum(sums, "Agent-notify-v9.9.9.zip").unwrap_err();
    assert_eq!(error.code(), "update_checksum_missing");
    assert!(error.message().contains("Agent-notify-v9.9.9.zip"));

    let short = "abc123  Agent-notify-v2.1.0.zip\n";
    let error = parse_checksum(short, "Agent-notify-v2.1.0.zip").unwrap_err();
    assert_eq!(error.code(), "update_checksum_invalid");
    assert!(error.message().contains("长度"));

    let not_hex = format!("{}  Agent-notify-v2.1.0.zip\n", "z".repeat(64));
    let error = parse_checksum(&not_hex, "Agent-notify-v2.1.0.zip").unwrap_err();
    assert_eq!(error.code(), "update_checksum_invalid");
    assert!(error.message().contains("格式"));
}

#[test]
fn checksum_mismatch_names_both_hashes() {
    let expected = "a".repeat(64);
    let actual = "b".repeat(64);
    let error = ensure_checksum_matches(&expected, &actual).unwrap_err();
    assert_eq!(error.code(), "update_checksum_mismatch");
    assert!(error.message().contains(&expected));
    assert!(error.message().contains(&actual));

    assert!(ensure_checksum_matches(&expected.to_uppercase(), &expected).is_ok());
}

#[test]
fn expected_checksum_reads_the_release_manifest() {
    let dir = test_dir("agentnotify-update-sums-");
    let sums_path = dir.path().join("SHA256SUMS.txt");
    std::fs::write(
        &sums_path,
        format!("{}  Agent-notify-v2.1.0.zip\n", "c".repeat(64)),
    )
    .expect("写入测试清单");

    assert_eq!(
        expected_checksum(&sums_path, "Agent-notify-v2.1.0.zip").expect("必须读取成功"),
        "c".repeat(64)
    );

    let missing =
        expected_checksum(&dir.path().join("missing.txt"), "Agent-notify-v2.1.0.zip").unwrap_err();
    assert_eq!(missing.code(), "update_checksums_unreadable");
}

#[test]
fn download_size_limit_rejects_oversized_content() {
    assert!(ensure_within_download_limit(0, 1024).is_ok());
    assert!(ensure_within_download_limit(1024, 1024).is_ok());
    let error = ensure_within_download_limit(1025, 1024).unwrap_err();
    assert_eq!(error.code(), "update_too_large");
    assert!(error.message().contains("1024"));
}

#[tokio::test]
async fn download_with_retry_recovers_from_transient_failures_and_cleans_part_files() {
    let dir = test_dir("agentnotify-update-retry-");
    let destination = dir.path().join("Agent-notify-Setup-v2.1.0.exe");
    let transport = FakeTransport::with_downloads(vec![
        DownloadOutcome::Fail(UpdateError::new("update_network_failed", "连接被重置")),
        DownloadOutcome::Fail(UpdateError::new("update_timeout", "请求超时")),
        DownloadOutcome::Write(b"MZ-fake-installer".to_vec()),
    ]);

    download_with_retry(
        &transport,
        "https://example.invalid/setup.exe",
        &destination,
        1024,
        DOWNLOAD_ATTEMPTS,
    )
    .await
    .expect("第三次尝试必须成功");

    assert_eq!(transport.attempts(), 3);
    assert_eq!(
        std::fs::read(&destination).expect("目标文件必须存在"),
        b"MZ-fake-installer"
    );
    assert!(
        !part_file(&destination).exists(),
        "成功后不能残留 .part 临时文件"
    );
}

#[tokio::test]
async fn download_with_retry_stops_after_the_configured_attempts_and_cleans_up() {
    let dir = test_dir("agentnotify-update-retry-fail-");
    let destination = dir.path().join("Agent-notify-v2.1.0.zip");
    let transport = FakeTransport::with_downloads(vec![
        DownloadOutcome::WriteThenFail(
            b"partial".to_vec(),
            UpdateError::new("update_too_large", "更新文件超过允许大小 8 字节"),
        ),
        DownloadOutcome::Fail(UpdateError::new(
            "update_too_large",
            "更新文件超过允许大小 8 字节",
        )),
        DownloadOutcome::Fail(UpdateError::new(
            "update_too_large",
            "更新文件超过允许大小 8 字节",
        )),
        DownloadOutcome::Fail(UpdateError::new(
            "update_too_large",
            "更新文件超过允许大小 8 字节",
        )),
    ]);

    let error = download_with_retry(
        &transport,
        "https://example.invalid/archive.zip",
        &destination,
        8,
        DOWNLOAD_ATTEMPTS,
    )
    .await
    .unwrap_err();

    assert_eq!(error.code(), "update_too_large");
    assert_eq!(transport.attempts(), 3, "必须按配置的次数重试");
    assert!(!destination.exists(), "失败时不得留下半截目标文件");
    assert!(!part_file(&destination).exists(), "失败时必须清理 .part");
}

fn part_file(destination: &Path) -> PathBuf {
    let mut value = destination.as_os_str().to_os_string();
    value.push(".part");
    PathBuf::from(value)
}
