use std::{fmt::Write as _, ptr::null_mut, sync::Arc};

use agentnotify_application::{SecretError, SecretKind, SecretStore, SecretValue};
use agentnotify_domain::ChannelAccountId;
use sha2::{Digest, Sha256};
use windows_sys::Win32::{
    Foundation::{ERROR_NOT_FOUND, GetLastError},
    Security::Credentials::{
        CRED_PERSIST_LOCAL_MACHINE, CRED_TYPE_GENERIC, CREDENTIALW, CredDeleteW, CredFree,
        CredReadW, CredWriteW,
    },
};

pub const CREDENTIAL_SERVICE_NAME: &str = "AgentNotify";

pub fn credential_target(account_id: &ChannelAccountId, kind: SecretKind) -> String {
    let mut digest = Sha256::new();
    digest.update(account_id.as_str().as_bytes());
    digest.update([0]);
    digest.update(kind.as_str().as_bytes());
    let digest = digest.finalize();
    let mut reference = String::with_capacity(digest.len() * 2);
    for byte in digest {
        write!(reference, "{byte:02x}").expect("写入 String 不会失败");
    }
    format!("{CREDENTIAL_SERVICE_NAME}/{reference}")
}

pub trait CredentialBackend: Send + Sync {
    fn read(&self, target: &str) -> Result<Option<SecretValue>, SecretError>;

    fn write(&self, target: &str, value: &SecretValue) -> Result<(), SecretError>;

    fn delete(&self, target: &str) -> Result<(), SecretError>;
}

#[derive(Clone)]
pub struct WindowsSecretStore {
    backend: Arc<dyn CredentialBackend>,
}

impl WindowsSecretStore {
    pub fn new() -> Self {
        Self::with_backend(Arc::new(WindowsCredentialBackend))
    }

    pub fn with_backend(backend: Arc<dyn CredentialBackend>) -> Self {
        Self { backend }
    }
}

impl Default for WindowsSecretStore {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl SecretStore for WindowsSecretStore {
    async fn get(
        &self,
        account_id: &ChannelAccountId,
        kind: SecretKind,
    ) -> Result<SecretValue, SecretError> {
        let target = credential_target(account_id, kind);
        self.backend.read(&target)?.ok_or_else(|| {
            SecretError::new("secret_not_found", "未找到该账号的渠道密钥，请重新登录")
        })
    }

    async fn set(
        &self,
        account_id: &ChannelAccountId,
        kind: SecretKind,
        value: SecretValue,
    ) -> Result<(), SecretError> {
        let target = credential_target(account_id, kind);
        self.backend.write(&target, &value)
    }

    async fn delete(
        &self,
        account_id: &ChannelAccountId,
        kind: SecretKind,
    ) -> Result<(), SecretError> {
        let target = credential_target(account_id, kind);
        self.backend.delete(&target)
    }
}

pub struct WindowsCredentialBackend;

impl CredentialBackend for WindowsCredentialBackend {
    fn read(&self, target: &str) -> Result<Option<SecretValue>, SecretError> {
        let target = wide_string(target);
        let mut credential = null_mut();
        if unsafe { CredReadW(target.as_ptr(), CRED_TYPE_GENERIC, 0, &mut credential) } == 0 {
            let error = unsafe { GetLastError() };
            if error == ERROR_NOT_FOUND {
                return Ok(None);
            }
            return Err(SecretError::new(
                "secret_read_failed",
                format!("Windows 凭据管理器读取失败，错误码 {error}"),
            ));
        }

        let credential = CredentialBuffer(credential);
        let record = unsafe { &*credential.0 };
        if record.CredentialBlobSize == 0 || record.CredentialBlob.is_null() {
            return Err(SecretError::new(
                "secret_invalid_value",
                "Windows 凭据管理器返回了空密钥",
            ));
        }
        let bytes = unsafe {
            std::slice::from_raw_parts(record.CredentialBlob, record.CredentialBlobSize as usize)
        };
        let value = std::str::from_utf8(bytes).map_err(|_| {
            SecretError::new(
                "secret_invalid_encoding",
                "Windows 凭据管理器中的密钥编码无效，请重新登录",
            )
        })?;
        SecretValue::new(value).map(Some)
    }

    fn write(&self, target: &str, value: &SecretValue) -> Result<(), SecretError> {
        let target_wide = wide_string(target);
        let mut value_bytes = value.expose().as_bytes().to_vec();
        let credential = credential_record(&target_wide, &mut value_bytes)?;

        if unsafe { CredWriteW(&credential, 0) } == 0 {
            let error = unsafe { GetLastError() };
            return Err(SecretError::new(
                "secret_write_failed",
                format!("Windows 凭据管理器写入失败，错误码 {error}"),
            ));
        }
        Ok(())
    }

    fn delete(&self, target: &str) -> Result<(), SecretError> {
        let target = wide_string(target);
        if unsafe { CredDeleteW(target.as_ptr(), CRED_TYPE_GENERIC, 0) } != 0 {
            return Ok(());
        }
        let error = unsafe { GetLastError() };
        if error == ERROR_NOT_FOUND {
            return Ok(());
        }
        Err(SecretError::new(
            "secret_delete_failed",
            format!("Windows 凭据管理器删除失败，错误码 {error}"),
        ))
    }
}

struct CredentialBuffer(*mut CREDENTIALW);

fn credential_record(target: &[u16], value: &mut [u8]) -> Result<CREDENTIALW, SecretError> {
    let value_length = u32::try_from(value.len()).map_err(|_| {
        SecretError::new(
            "secret_too_large",
            "渠道密钥过长，无法写入 Windows 凭据管理器",
        )
    })?;
    Ok(CREDENTIALW {
        Type: CRED_TYPE_GENERIC,
        TargetName: target.as_ptr().cast_mut(),
        CredentialBlobSize: value_length,
        CredentialBlob: value.as_mut_ptr(),
        Persist: CRED_PERSIST_LOCAL_MACHINE,
        ..CREDENTIALW::default()
    })
}

impl Drop for CredentialBuffer {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe {
                CredFree(self.0.cast());
            }
        }
    }
}

fn wide_string(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(Some(0)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn credential_record_uses_generic_type_without_real_credentials() {
        let target = wide_string("AgentNotify/test");
        let mut value = b"test-secret".to_vec();

        let record = credential_record(&target, &mut value).expect("测试密钥必须可转换为凭据记录");

        assert_eq!(record.Type, CRED_TYPE_GENERIC);
        assert_eq!(record.Persist, CRED_PERSIST_LOCAL_MACHINE);
        assert_eq!(record.CredentialBlobSize, value.len() as u32);
    }
}
