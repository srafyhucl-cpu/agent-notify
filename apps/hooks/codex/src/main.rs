#![cfg_attr(windows, windows_subsystem = "windows")]

//! Codex notify Hook 入口：读取有界 stdin，透传上游，再提交标准事件给 ingress。

use std::process::ExitCode;

use agentnotify_codex_hook::{RealPrograms, STDIN_TIMEOUT, read_bounded, run_hook};

fn main() -> ExitCode {
    let runtime = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        // 运行时都建不起来时按成功退出，不让 Hook 影响 Codex 的 notify 流程。
        Err(_) => return ExitCode::SUCCESS,
    };
    ExitCode::from(runtime.block_on(run()))
}

async fn run() -> u8 {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let stdin = read_stdin().await;
    run_hook(&args, &stdin, &RealPrograms).await.exit_code()
}

/// stdin 读取有上限也有超时：Codex 未关闭管道时 Hook 不能挂住。
async fn read_stdin() -> Vec<u8> {
    match tokio::time::timeout(STDIN_TIMEOUT, read_bounded(tokio::io::stdin())).await {
        Ok(Ok(buffer)) => buffer,
        _ => Vec::new(),
    }
}
