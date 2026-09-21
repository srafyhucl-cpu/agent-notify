#![cfg_attr(windows, windows_subsystem = "windows")]

//! Devin Stop Hook 入口：读取有界 stdin，提交标准事件给 ingress，
//! 并始终输出 Devin 要求的 `{}`。

use std::{io::Write, process::ExitCode};

use agentnotify_devin_hook::{RealPrograms, STDIN_TIMEOUT, STOP_RESPONSE, read_bounded, run_hook};

fn main() -> ExitCode {
    let runtime = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(_) => {
            // 运行时都建不起来时也必须输出 `{}`，不让 Hook 影响 Devin。
            write_stop_response();
            return ExitCode::SUCCESS;
        }
    };
    runtime.block_on(run());
    write_stop_response();
    ExitCode::SUCCESS
}

/// 通知失败只写诊断；Hook 始终按成功退出，不改变 Devin 的流程。
async fn run() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let stdin = read_stdin().await;
    run_hook(&args, &stdin, &RealPrograms).await;
}

/// Devin 要求 Stop Hook 输出 `{}`：解析、推送或 ingress 失败都不能改变这一点。
fn write_stop_response() {
    let mut stdout = std::io::stdout();
    let _ = stdout.write_all(STOP_RESPONSE.as_bytes());
    let _ = stdout.write_all(b"\n");
    let _ = stdout.flush();
}

/// stdin 读取有上限也有超时：Devin 未关闭管道时 Hook 不能挂住。
async fn read_stdin() -> Vec<u8> {
    match tokio::time::timeout(STDIN_TIMEOUT, read_bounded(tokio::io::stdin())).await {
        Ok(Ok(buffer)) => buffer,
        _ => Vec::new(),
    }
}
