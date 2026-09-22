use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use agentnotify_application::{SecretError, SecretKind, SecretStore, SecretValue};
use agentnotify_desktop::platform::windows::{
    AppPaths, CredentialBackend, ProcessOutcome, ProcessRequest, ProcessUnknownReason,
    WindowsBackgroundTasks, WindowsLocalIpc, WindowsProcessRunner, WindowsSecretStore,
    credential_target, validate_existing_directory,
};
use agentnotify_desktop::platform::{BackgroundTasks, LocalIpc, ProcessRunner};
use agentnotify_domain::ChannelAccountId;

const D_DRIVE_TEMP: &str = r"D:\Temp";

#[test]
fn test_overrides_isolate_all_platform_paths() {
    let paths = AppPaths::for_tests(Path::new(r"D:\Temp\agentnotify-tests"));
    assert!(paths.config_dir.starts_with(r"D:\Temp\agentnotify-tests"));
    assert!(paths.spool_dir.starts_with(r"D:\Temp\agentnotify-tests"));
    assert!(paths.log_dir.starts_with(r"D:\Temp\agentnotify-tests"));
    assert!(paths.data_dir.starts_with(r"D:\Temp\agentnotify-tests"));
    assert!(paths.temp_dir.starts_with(r"D:\Temp\agentnotify-tests"));
}

#[test]
fn app_paths_ensure_creates_all_isolated_directories() {
    let root = tempfile::Builder::new()
        .prefix("agentnotify-paths-")
        .tempdir_in(D_DRIVE_TEMP)
        .expect("D 盘测试目录必须可创建");
    let paths = AppPaths::for_tests(root.path());

    paths.ensure().expect("隔离路径必须可创建");

    for directory in [
        &paths.config_dir,
        &paths.data_dir,
        &paths.log_dir,
        &paths.spool_dir,
        &paths.temp_dir,
    ] {
        assert!(directory.is_dir(), "目录不存在: {}", directory.display());
    }
}

#[test]
fn credential_target_is_stable_and_does_not_embed_the_account_id() {
    let account = ChannelAccountId::new("account-with-sensitive-reference").expect("账号 ID 有效");

    let first = credential_target(&account, SecretKind::BotToken);
    let second = credential_target(&account, SecretKind::BotToken);

    assert_eq!(first, second);
    assert!(first.starts_with("AgentNotify/"));
    assert!(!first.contains(account.as_str()));
    assert_ne!(first, credential_target(&account, SecretKind::AppSecret));
}

#[tokio::test]
async fn secret_store_maps_missing_values_to_not_found_without_real_credentials() {
    let backend = Arc::new(MemoryCredentialBackend::default());
    let store = WindowsSecretStore::with_backend(backend);
    let account = ChannelAccountId::new("test-account").expect("账号 ID 有效");

    let error = store
        .get(&account, SecretKind::BotToken)
        .await
        .expect_err("缺失凭据必须返回错误");

    assert_eq!(error.code(), "secret_not_found");
    assert!(!error.message().is_empty());
}

#[tokio::test]
async fn secret_store_round_trips_through_injected_backend() {
    let backend = Arc::new(MemoryCredentialBackend::default());
    let store = WindowsSecretStore::with_backend(backend);
    let account = ChannelAccountId::new("test-account").expect("账号 ID 有效");
    let secret = SecretValue::new("test-secret-value").expect("密钥有效");

    store
        .set(&account, SecretKind::ContextToken, secret.clone())
        .await
        .expect("测试后端写入必须成功");
    let loaded = store
        .get(&account, SecretKind::ContextToken)
        .await
        .expect("测试后端读取必须成功");

    assert_eq!(loaded, secret);
}

#[tokio::test]
async fn process_runner_rejects_empty_program() {
    let runner = WindowsProcessRunner;
    let error = runner.run(ProcessRequest::new("")).await.unwrap_err();
    assert_eq!(error.code(), "process_program_empty");
}

#[tokio::test]
async fn process_runner_uses_explicit_environment_without_shell() {
    let runner = WindowsProcessRunner;
    let request = ProcessRequest::new(cmd_exe())
        .args([
            "/D",
            "/C",
            "echo %AGENT_NOTIFY_EXPLICIT%&if defined PATH exit /b 9",
        ])
        .env("AGENT_NOTIFY_EXPLICIT", "visible");

    let ProcessOutcome::Completed(output) = runner.run(request).await.expect("进程必须启动")
    else {
        panic!("普通命令不能返回 UnknownResult");
    };

    assert!(output.success());
    assert!(String::from_utf8_lossy(output.stdout().bytes()).contains("visible"));
    assert!(!String::from_utf8_lossy(output.stdout().bytes()).contains("PATH="));
}

#[tokio::test]
async fn process_runner_truncates_each_output_stream_at_the_limit() {
    let runner = WindowsProcessRunner;
    let request = ProcessRequest::new(cmd_exe()).args([
        "/D",
        "/C",
        "for /L %i in (1,1,30000) do @echo 012345678901234567890123456789",
    ]);

    let ProcessOutcome::Completed(output) = runner.run(request).await.expect("进程必须启动")
    else {
        panic!("普通命令不能返回 UnknownResult");
    };

    assert_eq!(output.stdout().bytes().len(), 256 * 1024);
    assert!(output.stdout().truncated());
}

#[tokio::test]
async fn process_runner_timeout_returns_unknown_result() {
    let runner = WindowsProcessRunner;
    let request = ProcessRequest::new(ping_exe())
        .args(["-n", "6", "127.0.0.1"])
        .timeout(Duration::from_millis(100));

    let outcome = runner.run(request).await.expect("超时必须表示为结果");

    assert_eq!(
        outcome,
        ProcessOutcome::UnknownResult {
            reason: ProcessUnknownReason::Timeout
        }
    );
}

#[tokio::test]
async fn process_runner_cancellation_returns_unknown_result() {
    let runner = WindowsProcessRunner;
    let (cancel, receiver) = tokio::sync::watch::channel(false);
    let request = ProcessRequest::new(ping_exe())
        .args(["-n", "6", "127.0.0.1"])
        .cancel_on(receiver);
    let run = tokio::spawn(async move { runner.run(request).await });

    tokio::time::sleep(Duration::from_millis(50)).await;
    cancel.send(true).expect("取消信号必须可发送");
    let outcome = run
        .await
        .expect("进程任务不应 panic")
        .expect("取消是正常结果");

    assert_eq!(
        outcome,
        ProcessOutcome::UnknownResult {
            reason: ProcessUnknownReason::Cancelled
        }
    );
}

#[tokio::test]
async fn background_tasks_signal_cooperative_cancellation() {
    let tasks = WindowsBackgroundTasks::default();
    tasks
        .spawn("cooperative", |cancel| {
            Box::pin(async move {
                cancel.cancelled().await;
            })
        })
        .expect("任务必须可登记");
    tokio::time::sleep(Duration::from_millis(20)).await;

    let snapshots = tasks.shutdown().await;
    let task = snapshots
        .iter()
        .find(|snapshot| snapshot.name == "cooperative")
        .expect("必须返回任务状态");
    assert_eq!(task.state.as_str(), "cancelled");
}

#[tokio::test]
async fn background_tasks_mark_uncooperative_tasks_unknown_after_five_seconds() {
    let tasks = WindowsBackgroundTasks::default();
    tasks
        .spawn("stubborn", |_| {
            Box::pin(async move {
                tokio::time::sleep(Duration::from_secs(30)).await;
            })
        })
        .expect("任务必须可登记");
    tokio::time::sleep(Duration::from_millis(20)).await;

    let started = Instant::now();
    let snapshots = tasks.shutdown().await;
    let elapsed = started.elapsed();

    assert!(elapsed >= Duration::from_secs(4), "过早结束: {elapsed:?}");
    assert!(
        elapsed < Duration::from_secs(7),
        "关闭耗时过长: {elapsed:?}"
    );
    let task = snapshots
        .iter()
        .find(|snapshot| snapshot.name == "stubborn")
        .expect("必须返回任务状态");
    assert_eq!(task.state.as_str(), "unknown");
}

#[tokio::test]
async fn local_ipc_rejects_invalid_pipe_name_before_connecting() {
    let ipc = WindowsLocalIpc::with_pipe_name("");
    let error = ipc
        .connect(b"{}", Duration::from_millis(10))
        .await
        .expect_err("空管道名必须拒绝");

    assert_eq!(error.code(), "ipc_invalid_name");
}

#[test]
fn system_ui_path_validation_rejects_paths_outside_app_paths() {
    let root = tempfile::Builder::new()
        .prefix("agentnotify-system-ui-")
        .tempdir_in(D_DRIVE_TEMP)
        .expect("D 盘测试目录必须可创建");
    let outside = tempfile::Builder::new()
        .prefix("agentnotify-system-ui-outside-")
        .tempdir_in(D_DRIVE_TEMP)
        .expect("D 盘测试目录必须可创建");
    let app_paths = AppPaths::for_tests(root.path());
    app_paths.ensure().expect("隔离路径必须可创建");

    let validated =
        validate_existing_directory(&app_paths, &app_paths.log_dir).expect("应用日志目录必须允许");
    assert!(validated.starts_with(root.path().canonicalize().unwrap()));

    let error = validate_existing_directory(&app_paths, outside.path())
        .expect_err("应用路径外目录必须拒绝");
    assert_eq!(error.code(), "system_ui_path_outside_app_paths");
}

#[test]
fn windows_platform_host_exposes_all_platform_ports_without_credentials_access() {
    let root = tempfile::Builder::new()
        .prefix("agentnotify-host-")
        .tempdir_in(D_DRIVE_TEMP)
        .expect("D 盘测试目录必须可创建");
    let host = agentnotify_desktop::platform::windows::WindowsPlatformHost::for_tests(root.path())
        .expect("测试宿主必须可创建");
    let paths = agentnotify_desktop::platform::PlatformHost::paths(&host);

    assert_eq!(
        paths.config_dir,
        AppPaths::for_tests(root.path()).config_dir
    );
    assert!(Arc::strong_count(&host.secret_store()) >= 1);
    assert!(Arc::strong_count(&host.process_runner()) >= 1);
    assert!(Arc::strong_count(&host.background_tasks()) >= 1);
    assert!(Arc::strong_count(&host.local_ipc()) >= 1);
    assert!(Arc::strong_count(&host.system_ui()) >= 1);
}

#[derive(Default)]
struct MemoryCredentialBackend {
    values: Mutex<BTreeMap<String, String>>,
}

impl CredentialBackend for MemoryCredentialBackend {
    fn read(&self, target: &str) -> Result<Option<SecretValue>, SecretError> {
        self.values
            .lock()
            .expect("测试凭据锁不应中毒")
            .get(target)
            .cloned()
            .map(SecretValue::new)
            .transpose()
    }

    fn write(&self, target: &str, value: &SecretValue) -> Result<(), SecretError> {
        self.values
            .lock()
            .expect("测试凭据锁不应中毒")
            .insert(target.to_owned(), value.expose().to_owned());
        Ok(())
    }

    fn delete(&self, target: &str) -> Result<(), SecretError> {
        self.values
            .lock()
            .expect("测试凭据锁不应中毒")
            .remove(target);
        Ok(())
    }
}

fn ping_exe() -> PathBuf {
    std::env::var_os("SystemRoot")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(r"C:\Windows"))
        .join("System32")
        .join("PING.EXE")
}

fn cmd_exe() -> PathBuf {
    std::env::var_os("COMSPEC")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(r"C:\Windows\System32\cmd.exe"))
}
