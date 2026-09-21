//! Hook 顺序契约与失败诊断：上游先运行，ingress 后提交；
//! ingress 失败只写诊断，不改上游退出语义，也不能静默消失。

use std::{
    process::ExitCode,
    sync::{Arc, Mutex},
    time::Duration,
};

use agentnotify_codex_hook::{
    DEBUG_LOG_MAX_BYTES, HookLimits, HookOutcome, HookPrograms, IngressFailure, MAX_STDIN_BYTES,
    PROTOCOL_VERSION, UpstreamFailure, append_debug_line_to, build_envelope,
    find_codex_computer_use_in, read_bounded, run_hook, run_hook_with_limits,
};

const CODEX_ARGS: [&str; 3] = [
    "codex",
    "turn-ended",
    r#"{"type":"agent-turn-complete","thread-id":"thread-1","last-assistant-message":"完成"}"#,
];
const CODEX_STDIN: &str =
    r#"{"type":"agent-turn-complete","thread-id":"thread-1","last-assistant-message":"完成"}"#;

#[derive(Default)]
struct FakePrograms {
    calls: Mutex<Vec<String>>,
    upstream_exit_code: i32,
    upstream_failure: Option<UpstreamFailure>,
    upstream_delay: Option<Duration>,
    ingress_failure: Option<IngressFailure>,
    ingress_delay: Option<Duration>,
    log_error: Option<String>,
    forwarded_args: Mutex<Vec<String>>,
    forwarded_stdin: Mutex<Vec<u8>>,
    envelope: Mutex<Option<serde_json::Value>>,
    diagnostics: Mutex<Vec<String>>,
}

#[async_trait::async_trait]
impl HookPrograms for FakePrograms {
    async fn run_upstream(&self, args: &[String], stdin: &[u8]) -> Result<i32, UpstreamFailure> {
        self.calls.lock().unwrap().push("upstream".to_owned());
        *self.forwarded_args.lock().unwrap() = args.to_vec();
        *self.forwarded_stdin.lock().unwrap() = stdin.to_vec();
        if let Some(delay) = self.upstream_delay {
            tokio::time::sleep(delay).await;
        }
        match &self.upstream_failure {
            Some(failure) => Err(failure.clone()),
            None => Ok(self.upstream_exit_code),
        }
    }

    async fn run_ingress(&self, envelope: &[u8]) -> Result<(), IngressFailure> {
        self.calls.lock().unwrap().push("ingress".to_owned());
        *self.envelope.lock().unwrap() = serde_json::from_slice(envelope).ok();
        if let Some(delay) = self.ingress_delay {
            tokio::time::sleep(delay).await;
        }
        match &self.ingress_failure {
            Some(failure) => Err(failure.clone()),
            None => Ok(()),
        }
    }

    async fn log_diagnostic(&self, message: &str) -> std::io::Result<()> {
        self.diagnostics.lock().unwrap().push(message.to_owned());
        match &self.log_error {
            Some(error) => Err(std::io::Error::other(error.clone())),
            None => Ok(()),
        }
    }
}

struct HookFixture {
    programs: Arc<FakePrograms>,
    outcome: Mutex<Option<HookOutcome>>,
}

fn codex_hook_fixture() -> HookFixture {
    HookFixture::with(FakePrograms::default())
}

impl HookFixture {
    fn with(programs: FakePrograms) -> Self {
        Self {
            programs: Arc::new(programs),
            outcome: Mutex::new(None),
        }
    }

    async fn handle(&self) {
        let args: Vec<String> = CODEX_ARGS.iter().map(|arg| (*arg).to_owned()).collect();
        self.handle_with(&args, CODEX_STDIN.as_bytes()).await;
    }

    async fn handle_with(&self, args: &[String], stdin: &[u8]) {
        let outcome = run_hook(args, stdin, self.programs.as_ref()).await;
        *self.outcome.lock().unwrap() = Some(outcome);
    }

    async fn handle_with_limits(&self, limits: HookLimits) {
        let args: Vec<String> = CODEX_ARGS.iter().map(|arg| (*arg).to_owned()).collect();
        let outcome = run_hook_with_limits(
            &args,
            CODEX_STDIN.as_bytes(),
            self.programs.as_ref(),
            limits,
        )
        .await;
        *self.outcome.lock().unwrap() = Some(outcome);
    }

    fn calls(&self) -> Vec<String> {
        self.programs.calls.lock().unwrap().clone()
    }

    fn exit_code(&self) -> ExitCode {
        ExitCode::from(self.exit_code_value())
    }

    fn exit_code_value(&self) -> u8 {
        self.outcome.lock().unwrap().as_ref().unwrap().exit_code()
    }

    fn forwarded_args(&self) -> Vec<String> {
        self.programs.forwarded_args.lock().unwrap().clone()
    }

    fn forwarded_stdin(&self) -> Vec<u8> {
        self.programs.forwarded_stdin.lock().unwrap().clone()
    }

    fn envelope(&self) -> serde_json::Value {
        self.programs
            .envelope
            .lock()
            .unwrap()
            .clone()
            .expect("ingress 必须收到事件")
    }

    fn diagnostics(&self) -> Vec<String> {
        self.programs.diagnostics.lock().unwrap().clone()
    }
}

#[tokio::test]
async fn upstream_runs_before_ingress_and_failure_does_not_block_upstream() {
    let fixture = HookFixture::with(FakePrograms {
        ingress_failure: Some(IngressFailure::Exit { code: Some(2) }),
        ..FakePrograms::default()
    });

    fixture.handle().await;

    assert_eq!(fixture.calls(), vec!["upstream", "ingress"]);
    assert_eq!(fixture.exit_code(), ExitCode::SUCCESS);
}

#[tokio::test]
async fn upstream_exit_code_is_propagated_to_hook() {
    let fixture = HookFixture::with(FakePrograms {
        upstream_exit_code: 7,
        ..FakePrograms::default()
    });

    fixture.handle().await;

    assert_eq!(fixture.exit_code_value(), 7);
}

#[tokio::test]
async fn missing_upstream_still_submits_ingress() {
    let fixture = HookFixture::with(FakePrograms {
        upstream_failure: Some(UpstreamFailure::NotFound),
        ..FakePrograms::default()
    });

    fixture.handle().await;

    assert_eq!(fixture.calls(), vec!["upstream", "ingress"]);
    assert_eq!(fixture.exit_code(), ExitCode::SUCCESS);
}

#[tokio::test]
async fn upstream_receives_original_args_and_stdin() {
    let fixture = codex_hook_fixture();

    fixture.handle().await;

    assert_eq!(
        fixture.forwarded_args(),
        vec!["turn-ended".to_owned(), CODEX_ARGS[2].to_owned()],
        "codex 选择参数由 Hook 消费，其余参数原样透传"
    );
    assert_eq!(fixture.forwarded_stdin(), CODEX_STDIN.as_bytes());
}

#[tokio::test]
async fn ingress_receives_versioned_codex_event() {
    let fixture = codex_hook_fixture();

    fixture.handle().await;

    let envelope = fixture.envelope();
    assert_eq!(envelope["protocolVersion"], PROTOCOL_VERSION);
    assert_eq!(envelope["kind"], "agent.event");
    assert_eq!(envelope["agentId"], "codex");
    let request_id = envelope["requestId"].as_str().unwrap();
    assert_eq!(
        uuid::Uuid::parse_str(request_id).unwrap().to_string(),
        request_id,
        "requestId 必须是合法 UUID"
    );
    assert_eq!(envelope["payload"]["thread-id"], "thread-1");
    assert_eq!(envelope["payload"]["last-assistant-message"], "完成");
}

#[tokio::test]
async fn stdin_payload_is_used_when_args_have_no_json() {
    let fixture = codex_hook_fixture();
    let args = vec!["codex".to_owned(), "turn-ended".to_owned()];

    fixture.handle_with(&args, CODEX_STDIN.as_bytes()).await;

    assert_eq!(fixture.calls(), vec!["upstream", "ingress"]);
    assert_eq!(fixture.envelope()["payload"]["thread-id"], "thread-1");
}

#[tokio::test]
async fn stdin_read_is_bounded() {
    let oversized = vec![b'x'; MAX_STDIN_BYTES + 1024];

    let buffer = read_bounded(oversized.as_slice()).await.unwrap();

    assert_eq!(buffer.len(), MAX_STDIN_BYTES);
}

#[test]
fn envelope_without_payload_still_submits_empty_event() {
    let bytes = build_envelope(&["codex".to_owned()], b"").unwrap();

    let envelope: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(envelope["payload"], serde_json::json!({}));
}

#[test]
fn upstream_discovery_picks_newest_runtime() {
    let root = tempfile::tempdir().unwrap();
    let older = write_fake_upstream(root.path(), "older");
    std::thread::sleep(Duration::from_millis(20));
    let newer = write_fake_upstream(root.path(), "newer");

    assert_eq!(
        find_codex_computer_use_in(root.path()),
        Some(newer),
        "必须选最新的 codex-computer-use.exe（旧版留在 {older:?}）"
    );
}

/// ingress 被协议拒绝（退出码 2）时必须留下可排查的诊断行。
#[tokio::test]
async fn ingress_rejection_is_logged_with_exit_code() {
    let fixture = HookFixture::with(FakePrograms {
        ingress_failure: Some(IngressFailure::Exit { code: Some(2) }),
        ..FakePrograms::default()
    });

    fixture.handle().await;

    let diagnostics = fixture.diagnostics();
    assert!(
        diagnostics.iter().any(|line| line.contains("ingress")
            && line.contains('2')
            && line.contains("协议拒绝")),
        "诊断必须带上退出码与原因：{diagnostics:?}"
    );
}

/// 上游与 ingress 都不可用时，两种原因都要写入诊断。
#[tokio::test]
async fn missing_upstream_and_ingress_are_logged() {
    let fixture = HookFixture::with(FakePrograms {
        upstream_failure: Some(UpstreamFailure::NotFound),
        ingress_failure: Some(IngressFailure::NotFound),
        ..FakePrograms::default()
    });

    fixture.handle().await;

    let diagnostics = fixture.diagnostics();
    assert!(
        diagnostics.iter().any(|line| line.contains("上游")),
        "{diagnostics:?}"
    );
    assert!(
        diagnostics
            .iter()
            .any(|line| line.contains("agentnotify-ingress.exe")),
        "{diagnostics:?}"
    );
}

/// 上游与 ingress 超时同样要写入诊断，且不能挂住 Hook。
#[tokio::test]
async fn upstream_and_ingress_timeouts_are_logged() {
    let fixture = HookFixture::with(FakePrograms {
        upstream_delay: Some(Duration::from_secs(5)),
        ingress_delay: Some(Duration::from_secs(5)),
        ..FakePrograms::default()
    });
    let limits = HookLimits {
        upstream_timeout: Duration::from_millis(20),
        ingress_timeout: Duration::from_millis(20),
    };

    fixture.handle_with_limits(limits).await;

    let diagnostics = fixture.diagnostics();
    assert!(
        diagnostics.iter().any(|line| line.contains("上游超时")),
        "{diagnostics:?}"
    );
    assert!(
        diagnostics
            .iter()
            .any(|line| line.contains("ingress 提交超时")),
        "{diagnostics:?}"
    );
}

/// 诊断写入失败（例如日志目录不可写）不得改变上游退出码或调用顺序。
#[tokio::test]
async fn diagnostic_failure_does_not_change_exit_code() {
    let fixture = HookFixture::with(FakePrograms {
        upstream_exit_code: 7,
        ingress_failure: Some(IngressFailure::Exit { code: Some(3) }),
        log_error: Some("日志目录不可写".to_owned()),
        ..FakePrograms::default()
    });

    fixture.handle().await;

    assert_eq!(fixture.exit_code_value(), 7);
    assert_eq!(fixture.calls(), vec!["upstream", "ingress"]);
    assert!(!fixture.diagnostics().is_empty(), "诊断写入仍被尝试过");
}

/// 每一种 ingress 失败都必须留下诊断，不允许静默消失。
#[tokio::test]
async fn every_ingress_failure_kind_is_logged() {
    let failures = [
        IngressFailure::NotFound,
        IngressFailure::SpawnFailed("boom".to_owned()),
        IngressFailure::WriteFailed("boom".to_owned()),
        IngressFailure::WaitFailed("boom".to_owned()),
        IngressFailure::Exit { code: Some(3) },
        IngressFailure::Exit { code: None },
        IngressFailure::Timeout,
        IngressFailure::Envelope("boom".to_owned()),
    ];

    for failure in failures {
        let fixture = HookFixture::with(FakePrograms {
            ingress_failure: Some(failure.clone()),
            ..FakePrograms::default()
        });

        fixture.handle().await;

        assert!(
            !fixture.diagnostics().is_empty(),
            "ingress 失败缺少诊断：{failure:?}"
        );
    }
}

/// 每一种上游失败都必须留下诊断。
#[tokio::test]
async fn every_upstream_failure_kind_is_logged() {
    let failures = [
        UpstreamFailure::NotFound,
        UpstreamFailure::SpawnFailed("boom".to_owned()),
        UpstreamFailure::WaitFailed("boom".to_owned()),
        UpstreamFailure::NoExitCode,
    ];

    for failure in failures {
        let fixture = HookFixture::with(FakePrograms {
            upstream_failure: Some(failure.clone()),
            ..FakePrograms::default()
        });

        fixture.handle().await;

        assert!(
            !fixture.diagnostics().is_empty(),
            "上游失败缺少诊断：{failure:?}"
        );
        assert_eq!(fixture.exit_code(), ExitCode::SUCCESS);
    }
}

/// 诊断文件超过上限时清空重写，且新失败一定留下原因。
#[test]
fn debug_log_is_bounded_and_contains_reason() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory
        .path()
        .join("agent-notify")
        .join("codex-notify-debug.log");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, vec![b'x'; DEBUG_LOG_MAX_BYTES as usize + 1]).unwrap();

    append_debug_line_to(&path, "ingress 退出码 2：事件被协议拒绝").unwrap();

    let content = std::fs::read_to_string(&path).unwrap();
    assert!(content.contains("ingress 退出码 2"), "{content}");
    assert!(
        (content.len() as u64) < DEBUG_LOG_MAX_BYTES,
        "超过上限后必须清空重写，实际 {} 字节",
        content.len()
    );
}

/// 诊断写入失败以 Err 暴露给调用方，由调用方吞掉（Hook 不允许影响主流程）。
#[test]
fn debug_log_write_failure_is_reported_to_caller() {
    let directory = tempfile::tempdir().unwrap();
    let blocker = directory.path().join("blocker");
    std::fs::write(&blocker, b"file").unwrap();

    let result = append_debug_line_to(&blocker.join("codex-notify-debug.log"), "x");

    assert!(result.is_err(), "父路径是文件时必须返回错误");
}

fn write_fake_upstream(local_app_data: &std::path::Path, runtime: &str) -> std::path::PathBuf {
    let directory = local_app_data
        .join("OpenAI")
        .join("Codex")
        .join("runtimes")
        .join("cua_node")
        .join(runtime)
        .join("bin")
        .join("node_modules")
        .join("@oai")
        .join("sky")
        .join("bin")
        .join("windows");
    std::fs::create_dir_all(&directory).unwrap();
    let path = directory.join("codex-computer-use.exe");
    std::fs::write(&path, b"fake").unwrap();
    path
}
