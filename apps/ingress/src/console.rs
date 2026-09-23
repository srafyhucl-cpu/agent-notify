//! windowsgui 二进制的诊断输出：把诊断文本显式接回父控制台。
//!
//! 复用 Go 版 `cmd/agent-notify` 的 bindConsole 语义：优先已重定向的 stdout，
//! 其次父控制台的 CONOUT$，都没有时新开一个控制台。只在 `--doctor` / `--ping` /
//! `--help` 分支调用，事件提交路径绝不触发，保持无控制台闪烁。

use std::io::{self, Write};

#[cfg(windows)]
use std::fs::OpenOptions;

#[cfg(windows)]
use windows_sys::Win32::{
    Foundation::{HANDLE, INVALID_HANDLE_VALUE},
    Storage::FileSystem::GetFileType,
    System::Console::{
        ATTACH_PARENT_PROCESS, AllocConsole, AttachConsole, GetConsoleWindow, GetStdHandle,
        STD_OUTPUT_HANDLE, SetConsoleOutputCP,
    },
};

/// 输出码页常量：报告含中文，按 UTF-8 写出避免按 ANSI 码页渲染成乱码。
#[cfg(windows)]
const UTF8_CODE_PAGE: u32 = 65001;

/// 返回诊断输出目标；Windows 下 stdout 不可用时接回父控制台。
pub fn diagnostic_writer() -> io::Result<Box<dyn Write>> {
    #[cfg(windows)]
    {
        if !stdout_usable() {
            ensure_console();
            unsafe {
                SetConsoleOutputCP(UTF8_CODE_PAGE);
            }
            return Ok(Box::new(OpenOptions::new().write(true).open("CONOUT$")?));
        }
    }
    Ok(Box::new(io::stdout()))
}

#[cfg(windows)]
fn stdout_usable() -> bool {
    unsafe {
        let handle: HANDLE = GetStdHandle(STD_OUTPUT_HANDLE);
        if handle.is_null() || handle == INVALID_HANDLE_VALUE {
            return false;
        }
        GetFileType(handle) != 0
    }
}

#[cfg(windows)]
fn ensure_console() {
    unsafe {
        if !GetConsoleWindow().is_null() {
            return;
        }
        if AttachConsole(ATTACH_PARENT_PROCESS) != 0 {
            return;
        }
        // 从资源管理器双击启动时没有父控制台：新开一个，用户至少能看到报告。
        let _ = AllocConsole();
    }
}

/// 把文本连同换行写入诊断输出并冲刷。
pub fn write_text(text: &str) -> io::Result<()> {
    let mut out = diagnostic_writer()?;
    out.write_all(text.as_bytes())?;
    out.write_all(b"\n")?;
    out.flush()
}
