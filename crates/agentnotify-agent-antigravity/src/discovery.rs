//! 本机 Antigravity 语言服务的发现边界：安装目录、PID、命令行里的 CSRF 令牌与监听端口。
//!
//! 发现结果一律是本机回环端点；读不到命令行或端口的进程直接跳过，绝不猜测目标。

use std::path::{Path, PathBuf};

use agentnotify_agent_sdk::AgentError;
use agentnotify_domain::SafeError;

use crate::reply::{EndpointDiscovery, LanguageServerEndpoint};

/// 语言服务可执行文件名，与 Go 版一致。
const LANGUAGE_SERVER_EXE_NAME: &str = "language_server.exe";
/// 显式覆盖语言服务路径的入口，便于自定义安装位置与隔离测试。
const LANGUAGE_SERVER_BINARY_ENV: &str = "AGENT_NOTIFY_ANTIGRAVITY_BIN";
const LANGUAGE_SERVER_BIN_SUBDIR: [&str; 2] = ["resources", "bin"];
/// Antigravity 安装根目录：`%LOCALAPPDATA%\Programs\antigravity` 与 `%APPDATA%\Programs\antigravity`。
const INSTALL_SUBDIR: [&str; 2] = ["Programs", "antigravity"];
const CSRF_TOKEN_FLAG: &str = "--csrf_token";
const LOCAL_APP_DATA_ENV: &str = "LOCALAPPDATA";
const ROAMING_APP_DATA_ENV: &str = "APPDATA";

/// `EndpointDiscovery` 的真实实现：先定位语言服务程序，再枚举它的进程与端口。
pub struct LanguageServerDiscovery;

impl LanguageServerDiscovery {
    pub fn from_default_location() -> Self {
        Self
    }
}

impl EndpointDiscovery for LanguageServerDiscovery {
    fn discover(&self) -> Result<Vec<LanguageServerEndpoint>, AgentError> {
        let binary = resolve_language_server_binary()?;
        Ok(discover_endpoints(&binary))
    }
}

/// 显式配置了 `AGENT_NOTIFY_ANTIGRAVITY_BIN` 就只认它（缺失即报错，不猜测回退）。
pub(crate) fn resolve_language_server_binary() -> Result<PathBuf, AgentError> {
    if let Some(configured) =
        std::env::var_os(LANGUAGE_SERVER_BINARY_ENV).filter(|value| !value.is_empty())
    {
        let path = PathBuf::from(configured);
        return if path.is_file() {
            Ok(path)
        } else {
            Err(unavailable(
                "antigravity_language_server_path_invalid",
                "Antigravity 语言服务路径不可用，请检查 AGENT_NOTIFY_ANTIGRAVITY_BIN 指向的文件",
            ))
        };
    }
    for candidate in language_server_candidates() {
        if candidate.is_file() {
            return Ok(candidate);
        }
    }
    Err(unavailable(
        "antigravity_language_server_missing",
        "未找到 Antigravity 桌面端的语言服务：请确认 Antigravity 桌面端已安装并正在运行",
    ))
}

fn language_server_candidates() -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    for key in [LOCAL_APP_DATA_ENV, ROAMING_APP_DATA_ENV] {
        let Some(root) = std::env::var_os(key).filter(|value| !value.is_empty()) else {
            continue;
        };
        candidates.push(
            PathBuf::from(root)
                .join(INSTALL_SUBDIR[0])
                .join(INSTALL_SUBDIR[1])
                .join(LANGUAGE_SERVER_BIN_SUBDIR[0])
                .join(LANGUAGE_SERVER_BIN_SUBDIR[1])
                .join(LANGUAGE_SERVER_EXE_NAME),
        );
    }
    candidates
}

/// 只保留与预期安装目录一致的语言服务进程，并用命令行令牌与监听端口组合出候选端点。
fn discover_endpoints(binary: &Path) -> Vec<LanguageServerEndpoint> {
    let Some(expected_install) = install_root_key(binary) else {
        return Vec::new();
    };
    let mut endpoints = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    for process in running_language_server_processes() {
        if install_root_key(&process.executable).as_deref() != Some(expected_install.as_str()) {
            continue;
        }
        if process.command_line.trim().is_empty() {
            continue;
        }
        let Some(token) = parse_command_line_flag(&process.command_line, CSRF_TOKEN_FLAG) else {
            continue;
        };
        for port in listening_ports(process.pid) {
            let endpoint = LanguageServerEndpoint::loopback(port, token.clone());
            if !endpoint.is_loopback() {
                continue;
            }
            if seen.insert(endpoint.address.clone()) {
                endpoints.push(endpoint);
            }
        }
    }
    endpoints
}

/// 安装根目录 = 可执行文件的爷爷目录；比较时忽略 Windows 路径大小写。
fn install_root_key(executable: &Path) -> Option<String> {
    let root = executable.parent()?.parent()?;
    Some(root.to_string_lossy().to_lowercase())
}

/// 读取 `--name value` 或 `--name=value` 形式的命令行参数。
fn parse_command_line_flag(command_line: &str, name: &str) -> Option<String> {
    let fields: Vec<&str> = command_line.split_whitespace().collect();
    for (index, field) in fields.iter().enumerate() {
        if *field == name {
            return fields.get(index + 1).map(|value| (*value).to_owned());
        }
        if let Some(value) = field.strip_prefix(&format!("{name}=")) {
            return Some(value.to_owned());
        }
    }
    None
}

pub(crate) struct RunningProcess {
    pub pid: u32,
    pub executable: PathBuf,
    pub command_line: String,
}

#[cfg(windows)]
mod platform {
    use std::{ffi::c_void, mem::size_of, path::PathBuf, ptr::null_mut};

    use windows_sys::Win32::{
        Foundation::{CloseHandle, HANDLE, INVALID_HANDLE_VALUE},
        NetworkManagement::IpHelper::{
            GetExtendedTcpTable, MIB_TCP_STATE_LISTEN, MIB_TCPROW_OWNER_PID,
            TCP_TABLE_OWNER_PID_LISTENER,
        },
        Networking::WinSock::AF_INET,
        System::{
            Diagnostics::{
                Debug::ReadProcessMemory,
                ToolHelp::{
                    CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW,
                    TH32CS_SNAPPROCESS,
                },
            },
            Threading::{
                OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_VM_READ,
                QueryFullProcessImageNameW,
            },
        },
    };

    use super::{LANGUAGE_SERVER_EXE_NAME, RunningProcess};

    /// 进程命令行只能从 PEB 读取；64 位偏移固定，32 位构建直接放弃，不做猜测。
    const PEB_PROCESS_PARAMETERS_OFFSET: usize = 0x20;
    const PARAMETERS_COMMAND_LINE_OFFSET: usize = 0x70;
    const PROCESS_BASIC_INFORMATION_CLASS: u32 = 0;
    const MAX_IMAGE_PATH_CHARS: usize = 32768;
    /// 命令行里最多解析的字节数，防止异常进程把解析拖死。
    const MAX_COMMAND_LINE_BYTES: usize = 64 * 1024;
    /// GetExtendedTcpTable 的初始缓冲：24 字节一行，按 64 条估算。
    const TCP_TABLE_INITIAL_BYTES: u32 = 24 * 64;

    #[repr(C)]
    #[derive(Default, Clone, Copy)]
    struct ProcessBasicInformation {
        reserved1: usize,
        peb_base_address: usize,
        reserved2: [usize; 2],
        unique_process_id: usize,
        reserved3: usize,
    }

    #[repr(C)]
    #[derive(Default, Clone, Copy)]
    struct UnicodeString {
        length: u16,
        maximum_length: u16,
        buffer: *mut u16,
    }

    #[link(name = "ntdll")]
    unsafe extern "system" {
        fn NtQueryInformationProcess(
            process_handle: HANDLE,
            process_information_class: u32,
            process_information: *mut c_void,
            process_information_length: u32,
            return_length: *mut u32,
        ) -> i32;
    }

    pub(super) fn running_language_server_processes() -> Vec<RunningProcess> {
        let mut processes = Vec::new();
        // SAFETY: 快照句柄在使用后关闭；缓冲区是本地栈对象，长度先写入 dwSize。
        unsafe {
            let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
            if snapshot.is_null() || snapshot == INVALID_HANDLE_VALUE {
                return processes;
            }
            let mut entry = PROCESSENTRY32W {
                dwSize: size_of::<PROCESSENTRY32W>() as u32,
                ..Default::default()
            };
            let mut has_entry = Process32FirstW(snapshot, &mut entry) != 0;
            while has_entry {
                let name = utf16_to_string(&entry.szExeFile);
                if name.eq_ignore_ascii_case(LANGUAGE_SERVER_EXE_NAME) {
                    let pid = entry.th32ProcessID;
                    if let Some(executable) = process_image_path(pid) {
                        let command_line = process_command_line(pid).unwrap_or_default();
                        processes.push(RunningProcess {
                            pid,
                            executable,
                            command_line,
                        });
                    }
                }
                has_entry = Process32NextW(snapshot, &mut entry) != 0;
            }
            CloseHandle(snapshot);
        }
        processes
    }

    fn process_image_path(pid: u32) -> Option<PathBuf> {
        // SAFETY: 句柄在函数结束前关闭；缓冲区长度通过 size 传入并由 API 回写实际长度。
        unsafe {
            let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
            if handle.is_null() {
                return None;
            }
            let mut buffer = vec![0u16; MAX_IMAGE_PATH_CHARS];
            let mut size = buffer.len() as u32;
            let ok = QueryFullProcessImageNameW(handle, 0, buffer.as_mut_ptr(), &mut size);
            CloseHandle(handle);
            if ok == 0 || size == 0 {
                return None;
            }
            Some(PathBuf::from(utf16_to_string(&buffer[..size as usize])))
        }
    }

    #[cfg(target_pointer_width = "64")]
    fn process_command_line(pid: u32) -> Option<String> {
        // SAFETY: 句柄在函数结束前关闭；远程读取只在有效句柄与检查过的地址上进行。
        unsafe {
            let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_VM_READ, 0, pid);
            if handle.is_null() {
                return None;
            }
            let result = read_remote_command_line(handle);
            CloseHandle(handle);
            result
        }
    }

    /// 32 位进程的内存布局与固定偏移不同，直接放弃读取命令行。
    #[cfg(not(target_pointer_width = "64"))]
    fn process_command_line(_pid: u32) -> Option<String> {
        None
    }

    #[cfg(target_pointer_width = "64")]
    unsafe fn read_remote_command_line(handle: HANDLE) -> Option<String> {
        let mut basic = ProcessBasicInformation::default();
        let mut returned = 0u32;
        let status = unsafe {
            NtQueryInformationProcess(
                handle,
                PROCESS_BASIC_INFORMATION_CLASS,
                &mut basic as *mut ProcessBasicInformation as *mut c_void,
                size_of::<ProcessBasicInformation>() as u32,
                &mut returned,
            )
        };
        if status < 0 || basic.peb_base_address == 0 {
            return None;
        }

        let parameters: usize = unsafe {
            read_remote(
                handle,
                basic.peb_base_address + PEB_PROCESS_PARAMETERS_OFFSET,
            )?
        };
        if parameters == 0 {
            return None;
        }
        let command_line: UnicodeString =
            unsafe { read_remote(handle, parameters + PARAMETERS_COMMAND_LINE_OFFSET)? };
        if command_line.buffer.is_null() || command_line.length == 0 {
            return None;
        }
        let bytes = normalized_command_line_bytes(usize::from(command_line.length))?;
        let mut buffer = vec![0u16; bytes / 2];
        unsafe {
            read_remote_bytes(
                handle,
                command_line.buffer as usize,
                buffer.as_mut_ptr() as *mut u8,
                bytes,
            )?;
        }
        Some(utf16_to_string(&buffer))
    }

    /// 归一化命令行长度：先截断到上限，再向下取偶，保证按 u16 读取时缓冲与字节数一致。
    fn normalized_command_line_bytes(length: usize) -> Option<usize> {
        let length = length.min(MAX_COMMAND_LINE_BYTES);
        let length = length & !1;
        (length > 0).then_some(length)
    }

    unsafe fn read_remote<T: Copy>(handle: HANDLE, address: usize) -> Option<T> {
        let mut value = std::mem::MaybeUninit::<T>::uninit();
        unsafe {
            read_remote_bytes(
                handle,
                address,
                value.as_mut_ptr() as *mut u8,
                size_of::<T>(),
            )?;
        }
        Some(unsafe { value.assume_init() })
    }

    unsafe fn read_remote_bytes(
        handle: HANDLE,
        address: usize,
        destination: *mut u8,
        size: usize,
    ) -> Option<()> {
        if size == 0 {
            return None;
        }
        let mut read = 0usize;
        let ok = unsafe {
            ReadProcessMemory(
                handle,
                address as *const c_void,
                destination as *mut c_void,
                size,
                &mut read,
            )
        };
        (ok != 0 && read == size).then_some(())
    }

    pub(super) fn listening_ports(pid: u32) -> Vec<u16> {
        if pid == 0 {
            return Vec::new();
        }
        // SAFETY: 缓冲区大小由 API 回写，读取前逐行检查剩余长度。
        unsafe {
            let mut size = 0u32;
            let _ = GetExtendedTcpTable(
                null_mut(),
                &mut size,
                0,
                u32::from(AF_INET),
                TCP_TABLE_OWNER_PID_LISTENER,
                0,
            );
            if size == 0 {
                size = TCP_TABLE_INITIAL_BYTES;
            }
            let mut buffer = vec![0u8; size as usize];
            let status = GetExtendedTcpTable(
                buffer.as_mut_ptr() as *mut c_void,
                &mut size,
                0,
                u32::from(AF_INET),
                TCP_TABLE_OWNER_PID_LISTENER,
                0,
            );
            if status != 0 || buffer.len() < size_of::<u32>() {
                return Vec::new();
            }
            let count = u32::from_ne_bytes([buffer[0], buffer[1], buffer[2], buffer[3]]) as usize;
            let row_size = size_of::<MIB_TCPROW_OWNER_PID>();
            let mut ports = Vec::new();
            let mut seen = std::collections::BTreeSet::new();
            for index in 0..count {
                let offset = size_of::<u32>() + index * row_size;
                if offset + row_size > buffer.len() {
                    break;
                }
                let row = std::ptr::read_unaligned(
                    buffer[offset..].as_ptr() as *const MIB_TCPROW_OWNER_PID
                );
                if row.dwState != MIB_TCP_STATE_LISTEN as u32 || row.dwOwningPid != pid {
                    continue;
                }
                // dwLocalPort 低 16 位是网络字节序的端口。
                let port = ((row.dwLocalPort >> 8) | ((row.dwLocalPort & 0xFF) << 8)) as u16;
                if port == 0 {
                    continue;
                }
                if seen.insert(port) {
                    ports.push(port);
                }
            }
            ports
        }
    }

    fn utf16_to_string(value: &[u16]) -> String {
        let end = value
            .iter()
            .position(|unit| *unit == 0)
            .unwrap_or(value.len());
        String::from_utf16_lossy(&value[..end])
    }
}

#[cfg(not(windows))]
mod platform {
    use super::RunningProcess;

    pub(super) fn running_language_server_processes() -> Vec<RunningProcess> {
        Vec::new()
    }

    pub(super) fn listening_ports(_pid: u32) -> Vec<u16> {
        Vec::new()
    }
}

fn running_language_server_processes() -> Vec<RunningProcess> {
    platform::running_language_server_processes()
}

fn listening_ports(pid: u32) -> Vec<u16> {
    platform::listening_ports(pid)
}

fn unavailable(code: &str, message: &str) -> AgentError {
    AgentError::Unavailable(
        SafeError::new(code, message).expect("Antigravity 错误常量必须是有效安全错误"),
    )
}
