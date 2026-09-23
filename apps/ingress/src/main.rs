#![cfg_attr(windows, windows_subsystem = "windows")]

use std::{
    io::{self, Read},
    process::ExitCode,
};

use agentnotify_ingress::{
    IngressError, IngressEvent, Spool, SpoolError, SpoolLimits, default_spool_dir,
    diagnostics::{DoctorReport, SELF_VERSION},
    protocol::MAX_PROTOCOL_BYTES,
};

#[cfg(windows)]
use agentnotify_ingress::{DEFAULT_CONNECT_TIMEOUT, SubmitResult, pipe_name, submit_with_fallback};

/// 退出码：0 正常；1 诊断异常；2 参数或协议错误；3 spool 错误；4 无可用输出。
const EXIT_UNHEALTHY: u8 = 1;
const EXIT_PROTOCOL: u8 = 2;
const EXIT_SPOOL: u8 = 3;
const EXIT_NO_OUTPUT: u8 = 4;

const ARG_DOCTOR: &str = "--doctor";
const ARG_PING: &str = "--ping";
const ARG_HELP: &str = "--help";

enum ReportStyle {
    /// 完整 JSON 报告。
    Full,
    /// 一行结论。
    Ping,
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let arg_refs: Vec<&str> = args.iter().map(String::as_str).collect();
    match arg_refs.as_slice() {
        // 无参数保持原协议：从 stdin 读版本化 agent.event（插件 / Hook 的调用方式）。
        [] => match run().await {
            Ok(()) => ExitCode::SUCCESS,
            Err(IngressExitError::Protocol) => ExitCode::from(EXIT_PROTOCOL),
            Err(IngressExitError::Spool) => ExitCode::from(EXIT_SPOOL),
        },
        [ARG_DOCTOR] => run_diagnostics(ReportStyle::Full),
        [ARG_PING] => run_diagnostics(ReportStyle::Ping),
        [ARG_HELP] | ["-h"] => finish(write_text(&usage_text()), ExitCode::SUCCESS),
        other => finish(
            write_text(&format!(
                "未知参数：{}（只支持 --doctor / --ping / --help）",
                other.join(" ")
            )),
            ExitCode::from(EXIT_PROTOCOL),
        ),
    }
}

async fn run() -> Result<(), IngressExitError> {
    let mut input = Vec::new();
    io::stdin()
        .take((MAX_PROTOCOL_BYTES + 1) as u64)
        .read_to_end(&mut input)
        .map_err(SpoolError::ReadFailed)?;
    let envelope = IngressEvent::parse(&input)?;
    let root = default_spool_dir().ok_or(SpoolError::InvalidPath)?;
    let spool = Spool::open(root, SpoolLimits::default())?;
    submit_event(&envelope, &spool).await?;
    Ok(())
}

/// 只读自检：不提交事件、不写盘，退出码反映健康与否。
fn run_diagnostics(style: ReportStyle) -> ExitCode {
    let report = DoctorReport::collect();
    let body = match style {
        ReportStyle::Full => match serde_json::to_string(&report) {
            Ok(json) => json,
            Err(error) => {
                return finish(
                    write_text(&format!("诊断报告序列化失败：{error}")),
                    ExitCode::from(EXIT_PROTOCOL),
                );
            }
        },
        ReportStyle::Ping => report.summary(),
    };
    if write_text(&body).is_err() {
        return ExitCode::from(EXIT_NO_OUTPUT);
    }
    if report.ok {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(EXIT_UNHEALTHY)
    }
}

fn usage_text() -> String {
    format!(
        "AgentNotify ingress v{} - 内部事件入口\n\
         \n\
         用法：\n\
         \x20 agentnotify-ingress            从 stdin 读取版本化 agent.event 并提交（插件 / Hook 调用）\n\
         \x20 agentnotify-ingress --doctor   只读自检，输出 JSON 报告（管道监听、spool 积压与隔离）\n\
         \x20 agentnotify-ingress --ping     只读探活，正常退出码 0，异常退出码 1\n\
         \x20 agentnotify-ingress --help     显示本帮助\n\
         \n\
         --doctor / --ping 只读，不提交事件、不写盘。\n\
         退出码：0 正常，1 诊断异常，2 参数或协议错误，3 spool 错误，4 无可用输出。",
        SELF_VERSION
    )
}

/// 诊断文本写完返回给定退出码；无可用输出时用 EXIT_NO_OUTPUT 明确暴露。
fn finish(result: io::Result<()>, code: ExitCode) -> ExitCode {
    match result {
        Ok(()) => code,
        Err(_) => ExitCode::from(EXIT_NO_OUTPUT),
    }
}

fn write_text(text: &str) -> io::Result<()> {
    agentnotify_ingress::console::write_text(text)
}

enum IngressExitError {
    Protocol,
    Spool,
}

impl From<IngressError> for IngressExitError {
    fn from(_value: IngressError) -> Self {
        Self::Protocol
    }
}

impl From<SpoolError> for IngressExitError {
    fn from(_value: SpoolError) -> Self {
        Self::Spool
    }
}

#[cfg(windows)]
async fn submit_event(
    envelope: &agentnotify_agent_sdk::AgentEventEnvelope,
    spool: &Spool,
) -> Result<SubmitResult, SpoolError> {
    let name = pipe_name().ok();
    submit_with_fallback(envelope, name.as_deref(), spool, DEFAULT_CONNECT_TIMEOUT).await
}

#[cfg(not(windows))]
async fn submit_event(
    envelope: &agentnotify_agent_sdk::AgentEventEnvelope,
    spool: &Spool,
) -> Result<(), SpoolError> {
    spool.write_event(envelope)?;
    Ok(())
}
