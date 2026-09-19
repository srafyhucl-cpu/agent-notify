use std::{io, process::Stdio};

use tokio::{
    io::{AsyncRead, AsyncReadExt},
    process::Command,
};
use windows_sys::Win32::System::Threading::CREATE_NO_WINDOW;

use super::{
    CapturedOutput, ProcessError, ProcessOutcome, ProcessOutput, ProcessRequest,
    ProcessUnknownReason,
};

pub const MAX_PROCESS_OUTPUT_BYTES: usize = 256 * 1024;

const OUTPUT_READ_BUFFER_BYTES: usize = 8 * 1024;

#[derive(Clone, Copy, Debug, Default)]
pub struct WindowsProcessRunner;

#[async_trait::async_trait]
impl super::ProcessRunner for WindowsProcessRunner {
    async fn run(&self, request: ProcessRequest) -> Result<ProcessOutcome, ProcessError> {
        if request.program.as_os_str().is_empty() {
            return Err(ProcessError::new(
                "process_program_empty",
                "进程程序路径不能为空",
            ));
        }

        let mut command = Command::new(&request.program);
        command
            .args(&request.args)
            .env_clear()
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .creation_flags(CREATE_NO_WINDOW);

        for name in &request.env_allowlist {
            if let Some(value) = std::env::var_os(name) {
                command.env(name, value);
            }
        }
        for (name, value) in &request.env {
            command.env(name, value);
        }
        if let Some(directory) = &request.cwd {
            command.current_dir(directory);
        }

        let mut child = command.spawn().map_err(|error| {
            ProcessError::new(
                "process_spawn_failed",
                format!("无法启动进程 {}：{error}", request.program.display()),
            )
        })?;
        let stdout = child.stdout.take().ok_or_else(|| {
            ProcessError::new("process_stdout_unavailable", "无法捕获进程标准输出")
        })?;
        let stderr = child.stderr.take().ok_or_else(|| {
            ProcessError::new("process_stderr_unavailable", "无法捕获进程标准错误")
        })?;

        let operation = async {
            let (status, stdout, stderr) = tokio::try_join!(
                child.wait(),
                read_capped(stdout, MAX_PROCESS_OUTPUT_BYTES),
                read_capped(stderr, MAX_PROCESS_OUTPUT_BYTES),
            )
            .map_err(|error| {
                ProcessError::new("process_io_failed", format!("读取进程结果失败：{error}"))
            })?;
            Ok::<_, ProcessError>(ProcessOutcome::Completed(ProcessOutput::new(
                status.code(),
                status.success(),
                stdout,
                stderr,
            )))
        };
        tokio::pin!(operation);

        match request.cancel {
            Some(mut cancel) => {
                if *cancel.borrow() {
                    return Ok(unknown_result(ProcessUnknownReason::Cancelled));
                }
                tokio::select! {
                    result = tokio::time::timeout(request.timeout, &mut operation) => {
                        match result {
                            Ok(result) => result,
                            Err(_) => Ok(unknown_result(ProcessUnknownReason::Timeout)),
                        }
                    }
                    changed = cancel.changed() => {
                        let _ = changed;
                        Ok(unknown_result(ProcessUnknownReason::Cancelled))
                    }
                }
            }
            None => match tokio::time::timeout(request.timeout, operation).await {
                Ok(result) => result,
                Err(_) => Ok(unknown_result(ProcessUnknownReason::Timeout)),
            },
        }
    }
}

fn unknown_result(reason: ProcessUnknownReason) -> ProcessOutcome {
    ProcessOutcome::UnknownResult { reason }
}

async fn read_capped<R>(mut reader: R, limit: usize) -> io::Result<CapturedOutput>
where
    R: AsyncRead + Unpin,
{
    let mut output = Vec::with_capacity(limit.min(OUTPUT_READ_BUFFER_BYTES));
    let mut truncated = false;
    let mut buffer = [0_u8; OUTPUT_READ_BUFFER_BYTES];

    loop {
        let read = reader.read(&mut buffer).await?;
        if read == 0 {
            break;
        }
        let remaining = limit.saturating_sub(output.len());
        let retained = remaining.min(read);
        output.extend_from_slice(&buffer[..retained]);
        if retained < read {
            truncated = true;
        }
    }

    Ok(CapturedOutput::new(output, truncated))
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    #[tokio::test]
    async fn capped_reader_drains_after_reaching_the_limit() {
        let input = vec![b'x'; MAX_PROCESS_OUTPUT_BYTES + 100];
        let output = read_capped(input.as_slice(), MAX_PROCESS_OUTPUT_BYTES)
            .await
            .expect("内存读取不会失败");

        assert_eq!(output.bytes().len(), MAX_PROCESS_OUTPUT_BYTES);
        assert!(output.truncated());
    }

    #[test]
    fn timeout_and_cancellation_are_unknown_results() {
        assert!(unknown_result(ProcessUnknownReason::Timeout).is_unknown());
        assert!(unknown_result(ProcessUnknownReason::Cancelled).is_unknown());
    }

    #[test]
    fn default_timeout_is_bounded() {
        assert_eq!(
            ProcessRequest::new("test.exe").timeout,
            Duration::from_secs(30)
        );
    }
}
