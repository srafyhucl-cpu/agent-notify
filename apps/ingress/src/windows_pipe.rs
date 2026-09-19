use std::{io, os::windows::io::RawHandle, ptr::null_mut, sync::Arc, time::Duration};

use agentnotify_agent_sdk::AgentEventEnvelope;
use sha2::{Digest, Sha256};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::windows::named_pipe::{ClientOptions, NamedPipeServer},
    sync::{Semaphore, watch},
    time::timeout,
};
use windows_sys::Win32::{
    Foundation::{
        CloseHandle, ERROR_ACCESS_DENIED, GetLastError, HANDLE, INVALID_HANDLE_VALUE, LocalFree,
    },
    Security::Authorization::{
        ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW,
        SDDL_REVISION_1,
    },
    Security::{
        GetTokenInformation, PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES, TOKEN_QUERY, TOKEN_USER,
        TokenUser,
    },
    Storage::FileSystem::{
        FILE_FLAG_FIRST_PIPE_INSTANCE, FILE_FLAG_OVERLAPPED, PIPE_ACCESS_DUPLEX,
    },
    System::Pipes::{
        CreateNamedPipeW, PIPE_READMODE_MESSAGE, PIPE_REJECT_REMOTE_CLIENTS, PIPE_TYPE_MESSAGE,
        PIPE_WAIT,
    },
    System::Threading::{GetCurrentProcess, OpenProcessToken},
};

use crate::protocol::{IngressError, IngressEvent, MAX_PROTOCOL_BYTES};
use crate::spool::{Spool, SpoolError};

pub const PIPE_NAME_PREFIX: &str = r"\\.\pipe\agentnotify-v1-";
pub const DEFAULT_CONNECT_TIMEOUT: Duration = Duration::from_millis(150);
const MAX_CONNECTIONS: usize = 8;
const MAX_PIPE_INSTANCES: usize = MAX_CONNECTIONS + 1;
const PIPE_IO_TIMEOUT: Duration = Duration::from_secs(5);
const PIPE_BUFFER_BYTES: u32 = MAX_PROTOCOL_BYTES as u32;
const ACK_ACCEPTED: u8 = 0;
const ACK_RETRY: u8 = 1;
const ACK_INVALID: u8 = 2;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SubmitResult {
    Submitted,
    Spooled,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HandlerError {
    code: &'static str,
    message: &'static str,
}

impl HandlerError {
    pub const fn new(code: &'static str, message: &'static str) -> Self {
        Self { code, message }
    }

    pub const fn code(self) -> &'static str {
        self.code
    }

    pub const fn message(self) -> &'static str {
        self.message
    }
}

#[async_trait::async_trait]
pub trait IngressHandler: Send + Sync {
    async fn handle(&self, envelope: AgentEventEnvelope) -> Result<(), HandlerError>;
}

#[derive(Debug, thiserror::Error)]
pub enum PipeError {
    #[error("当前用户 SID 无法读取")]
    SidUnavailable,
    #[error("命名管道安全描述符无法创建")]
    SecurityDescriptorUnavailable,
    #[error("命名管道名称无效")]
    InvalidName,
    #[error("已有 AgentNotify 核心正在监听命名管道")]
    AlreadyRunning,
    #[error("命名管道创建失败")]
    CreateFailed(u32),
    #[error("命名管道操作失败")]
    Io(#[from] io::Error),
    #[error("入口事件协议无效")]
    Protocol(#[from] IngressError),
}

impl PipeError {
    pub const fn code(&self) -> &'static str {
        match self {
            Self::SidUnavailable => "pipe_sid_unavailable",
            Self::SecurityDescriptorUnavailable => "pipe_security_descriptor_unavailable",
            Self::InvalidName => "pipe_name_invalid",
            Self::AlreadyRunning => "pipe_already_running",
            Self::CreateFailed(_) => "pipe_create_failed",
            Self::Io(_) => "pipe_io_failed",
            Self::Protocol(_) => "pipe_protocol_error",
        }
    }

    pub const fn message(&self) -> &'static str {
        match self {
            Self::SidUnavailable => "当前用户 SID 无法读取",
            Self::SecurityDescriptorUnavailable => "命名管道安全描述符无法创建",
            Self::InvalidName => "命名管道名称无效",
            Self::AlreadyRunning => "已有 AgentNotify 核心正在监听命名管道",
            Self::CreateFailed(_) => "命名管道创建失败",
            Self::Io(_) => "命名管道操作失败",
            Self::Protocol(_) => "入口事件协议无效",
        }
    }
}

pub fn pipe_name() -> Result<String, PipeError> {
    let sid = current_user_sid_string()?;
    let digest = Sha256::digest(sid.as_bytes());
    let mut suffix = String::with_capacity(16);
    for byte in &digest[..8] {
        use std::fmt::Write as _;
        write!(suffix, "{byte:02x}").expect("写入 String 不会失败");
    }
    Ok(format!("{PIPE_NAME_PREFIX}{suffix}"))
}

pub fn current_user_sddl() -> Result<String, PipeError> {
    let sid = current_user_sid_string()?;
    Ok(format!("D:P(A;;GA;;;{sid})"))
}

pub async fn serve(
    handler: Arc<dyn IngressHandler>,
    cancel: watch::Receiver<bool>,
) -> Result<(), PipeError> {
    serve_on(pipe_name()?, handler, cancel).await
}

pub async fn serve_on(
    name: String,
    handler: Arc<dyn IngressHandler>,
    mut cancel: watch::Receiver<bool>,
) -> Result<(), PipeError> {
    if !name.starts_with(PIPE_NAME_PREFIX) || name.contains('\0') {
        return Err(PipeError::InvalidName);
    }
    let connections = Arc::new(Semaphore::new(MAX_CONNECTIONS));
    let mut listener = create_server_instance(&name, true)?;

    loop {
        let permit = tokio::select! {
            permit = connections.clone().acquire_owned() => {
                permit.map_err(|_| PipeError::Io(io::Error::other("pipe 连接信号量不可用")))?
            }
            changed = cancel.changed() => {
                changed.map_err(|error| PipeError::Io(io::Error::other(error)))?;
                if *cancel.borrow() {
                    return Ok(());
                }
                continue;
            }
        };
        tokio::select! {
            result = listener.connect() => {
                result.map_err(PipeError::Io)?;
            }
            changed = cancel.changed() => {
                changed.map_err(|error| PipeError::Io(io::Error::other(error)))?;
                if *cancel.borrow() {
                    return Ok(());
                }
                continue;
            }
        }

        let connected = listener;
        let connection_handler = handler.clone();
        tokio::spawn(async move {
            let _permit = permit;
            let _ = timeout(
                PIPE_IO_TIMEOUT,
                handle_connection(connected, connection_handler),
            )
            .await;
        });
        listener = create_server_instance(&name, false)?;
    }
}

pub async fn connect_and_submit(
    name: &str,
    payload: &[u8],
    connect_timeout: Duration,
) -> Result<SubmitResult, PipeError> {
    if !name.starts_with(PIPE_NAME_PREFIX) || name.contains('\0') {
        return Err(PipeError::InvalidName);
    }
    connect_and_submit_inner(name, payload, connect_timeout).await
}

pub async fn submit_with_fallback(
    envelope: &AgentEventEnvelope,
    name: Option<&str>,
    spool: &Spool,
    connect_timeout: Duration,
) -> Result<SubmitResult, SpoolError> {
    let acknowledged = match name {
        Some(name) => {
            let payload =
                crate::protocol::encode_envelope(envelope).map_err(SpoolError::EncodeFailed)?;
            matches!(
                connect_and_submit(name, &payload, connect_timeout).await,
                Ok(SubmitResult::Submitted)
            )
        }
        None => false,
    };
    if acknowledged {
        return Ok(SubmitResult::Submitted);
    }
    spool.write_event(envelope)?;
    Ok(SubmitResult::Spooled)
}

async fn connect_and_submit_inner(
    name: &str,
    payload: &[u8],
    connect_timeout: Duration,
) -> Result<SubmitResult, PipeError> {
    let mut client = match timeout(connect_timeout, async {
        ClientOptions::new().open(name).map_err(PipeError::Io)
    })
    .await
    {
        Ok(Ok(client)) => client,
        Ok(Err(error)) => return Err(error),
        Err(_) => {
            return Err(PipeError::Io(io::Error::new(
                io::ErrorKind::TimedOut,
                "连接超时",
            )));
        }
    };

    timeout(PIPE_IO_TIMEOUT, write_frame(&mut client, payload))
        .await
        .map_err(|_| PipeError::Io(io::Error::new(io::ErrorKind::TimedOut, "写入超时")))??;
    let acknowledgement = timeout(PIPE_IO_TIMEOUT, read_acknowledgement(&mut client))
        .await
        .map_err(|_| PipeError::Io(io::Error::new(io::ErrorKind::TimedOut, "确认超时")))??;
    match acknowledgement {
        ACK_ACCEPTED => Ok(SubmitResult::Submitted),
        ACK_RETRY => Ok(SubmitResult::Spooled),
        ACK_INVALID => Err(PipeError::Protocol(IngressError::InvalidJson)),
        _ => Err(PipeError::Io(io::Error::new(
            io::ErrorKind::InvalidData,
            "未知的管道确认码",
        ))),
    }
}

async fn handle_connection(
    mut connection: NamedPipeServer,
    handler: Arc<dyn IngressHandler>,
) -> Result<(), PipeError> {
    let payload = read_frame(&mut connection).await?;
    let acknowledgement = match IngressEvent::parse(&payload) {
        Ok(envelope) => match handler.handle(envelope).await {
            Ok(()) => ACK_ACCEPTED,
            Err(_) => ACK_RETRY,
        },
        Err(_) => ACK_INVALID,
    };
    connection.write_all(&[acknowledgement]).await?;
    connection.flush().await?;
    Ok(())
}

async fn read_frame(stream: &mut NamedPipeServer) -> Result<Vec<u8>, PipeError> {
    let length = stream.read_u32().await?;
    if length as usize > MAX_PROTOCOL_BYTES {
        return Err(PipeError::Io(io::Error::new(
            io::ErrorKind::InvalidData,
            "入口事件超过长度上限",
        )));
    }
    let mut payload = vec![0_u8; length as usize];
    stream.read_exact(&mut payload).await?;
    Ok(payload)
}

async fn write_frame(
    stream: &mut tokio::net::windows::named_pipe::NamedPipeClient,
    payload: &[u8],
) -> Result<(), PipeError> {
    if payload.len() > MAX_PROTOCOL_BYTES {
        return Err(PipeError::Io(io::Error::new(
            io::ErrorKind::InvalidData,
            "入口事件超过长度上限",
        )));
    }
    stream.write_u32(payload.len() as u32).await?;
    stream.write_all(payload).await?;
    stream.flush().await?;
    Ok(())
}

async fn read_acknowledgement(
    stream: &mut tokio::net::windows::named_pipe::NamedPipeClient,
) -> Result<u8, PipeError> {
    Ok(stream.read_u8().await?)
}

fn create_server_instance(name: &str, first: bool) -> Result<NamedPipeServer, PipeError> {
    let name = wide_string(name);
    let security = SecurityDescriptor::for_current_user()?;
    let attributes = SECURITY_ATTRIBUTES {
        nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: security.0,
        bInheritHandle: 0,
    };
    let open_mode = PIPE_ACCESS_DUPLEX
        | FILE_FLAG_OVERLAPPED
        | if first {
            FILE_FLAG_FIRST_PIPE_INSTANCE
        } else {
            0
        };
    let pipe_mode =
        PIPE_TYPE_MESSAGE | PIPE_READMODE_MESSAGE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS;
    let handle = unsafe {
        CreateNamedPipeW(
            name.as_ptr(),
            open_mode,
            pipe_mode,
            MAX_PIPE_INSTANCES as u32,
            PIPE_BUFFER_BYTES,
            PIPE_BUFFER_BYTES,
            0,
            &attributes,
        )
    };
    if handle == INVALID_HANDLE_VALUE {
        let error = unsafe { GetLastError() };
        if first && error == ERROR_ACCESS_DENIED {
            return Err(PipeError::AlreadyRunning);
        }
        return Err(PipeError::CreateFailed(error));
    }

    let owned = OwnedHandle(handle);
    let server =
        unsafe { NamedPipeServer::from_raw_handle(handle as RawHandle) }.map_err(PipeError::Io)?;
    std::mem::forget(owned);
    Ok(server)
}

fn current_user_sid_string() -> Result<String, PipeError> {
    unsafe {
        let mut token: HANDLE = null_mut();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
            return Err(PipeError::SidUnavailable);
        }
        let token = OwnedHandle(token);

        let mut required = 0_u32;
        GetTokenInformation(token.0, TokenUser, null_mut(), 0, &mut required);
        GetTokenInformation(token.0, TokenUser, null_mut(), 0, &mut required);
        if required == 0 {
            return Err(PipeError::SidUnavailable);
        }
        let mut buffer = vec![0_u8; required as usize];
        if {
            GetTokenInformation(
                token.0,
                TokenUser,
                buffer.as_mut_ptr().cast(),
                required,
                &mut required,
            )
        } == 0
        {
            return Err(PipeError::SidUnavailable);
        }
        let token_user = &*(buffer.as_ptr().cast::<TOKEN_USER>());
        let mut sid_text = null_mut();
        if ConvertSidToStringSidW(token_user.User.Sid, &mut sid_text) == 0 {
            return Err(PipeError::SidUnavailable);
        }
        let sid = wide_pointer_to_string(sid_text);
        LocalFree(sid_text.cast());
        Ok(sid)
    }
}

struct SecurityDescriptor(PSECURITY_DESCRIPTOR);

impl SecurityDescriptor {
    fn for_current_user() -> Result<Self, PipeError> {
        let sddl = wide_string(&current_user_sddl()?);
        let mut descriptor = null_mut();
        let mut descriptor_size = 0_u32;
        if unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                sddl.as_ptr(),
                SDDL_REVISION_1,
                &mut descriptor,
                &mut descriptor_size,
            )
        } == 0
        {
            return Err(PipeError::SecurityDescriptorUnavailable);
        }
        Ok(Self(descriptor))
    }
}

impl Drop for SecurityDescriptor {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe {
                LocalFree(self.0.cast());
            }
        }
    }
}

struct OwnedHandle(HANDLE);

impl Drop for OwnedHandle {
    fn drop(&mut self) {
        if !self.0.is_null() && self.0 != INVALID_HANDLE_VALUE {
            unsafe {
                CloseHandle(self.0);
            }
        }
    }
}

fn wide_string(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(Some(0)).collect()
}

fn wide_pointer_to_string(value: *const u16) -> String {
    if value.is_null() {
        return String::new();
    }
    let mut length = 0;
    unsafe {
        while *value.add(length) != 0 {
            length += 1;
        }
        String::from_utf16_lossy(std::slice::from_raw_parts(value, length))
    }
}
