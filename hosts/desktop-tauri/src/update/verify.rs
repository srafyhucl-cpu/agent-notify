use std::{
    fs::File,
    io::{Read, Seek, SeekFrom},
    path::Path,
};

use sha2::{Digest, Sha256};

const HASH_HEX_LENGTH: usize = 64;
const DOS_HEADER_MIN_LENGTH: usize = 64;
const DOS_MZ_SIGNATURE: [u8; 2] = *b"MZ";
const PE_SIGNATURE: [u8; 4] = *b"PE\0\0";
const PE_OFFSET_LOCATION: usize = 0x3c;
const PE_COFF_HEADER_LENGTH: usize = 20;
const PE64_OPTIONAL_MAGIC: u16 = 0x020b;

/// 更新包签名策略。正式通道必须使用 `Required`，预览包可以显式使用 `Optional`。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SignatureRequirement {
    Required,
    Optional,
}

/// 更新包校验失败。错误码稳定，文案可以直接交给用户。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UpdateVerificationError {
    code: &'static str,
    message: String,
}

impl UpdateVerificationError {
    fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }

    pub fn code(&self) -> &'static str {
        self.code
    }

    pub fn message(&self) -> &str {
        &self.message
    }
}

impl std::fmt::Display for UpdateVerificationError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.message())
    }
}

impl std::error::Error for UpdateVerificationError {}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VerifiedUpdate {
    pub sha256: String,
    pub version: Option<String>,
    pub signed: bool,
}

/// 校验下载产物；顺序固定为 SHA-256、PE、文件版本和 Authenticode。
pub fn verify_download(
    path: &Path,
    expected_sha256: &str,
    signature_requirement: SignatureRequirement,
    expected_version: Option<&str>,
) -> Result<VerifiedUpdate, UpdateVerificationError> {
    let expected_hash = normalize_expected_sha256(expected_sha256)?;
    let actual_hash = sha256_file(path)?;
    if actual_hash != expected_hash {
        return Err(UpdateVerificationError::new(
            "update_checksum_mismatch",
            "更新包校验失败，下载内容与发布清单不一致。",
        ));
    }

    let pe = inspect_pe(path)?;
    if let Some(expected) = expected_version {
        let actual = pe.version.as_deref().ok_or_else(|| {
            UpdateVerificationError::new(
                "update_version_unavailable",
                "更新包缺少可读取的文件版本信息，已拒绝安装。",
            )
        })?;
        if !version_matches(expected, actual) {
            return Err(UpdateVerificationError::new(
                "update_version_mismatch",
                format!("更新包版本不匹配：期望 {expected}，实际 {actual}。"),
            ));
        }
    }

    let signature = authenticode_status(path)?;
    let signed = match signature {
        AuthenticodeStatus::Trusted => true,
        AuthenticodeStatus::Missing if signature_requirement == SignatureRequirement::Optional => {
            false
        }
        AuthenticodeStatus::Missing => {
            return Err(UpdateVerificationError::new(
                "update_signature_missing",
                "更新包未签名，已按正式通道策略拒绝安装。",
            ));
        }
        AuthenticodeStatus::Invalid(status) => {
            return Err(UpdateVerificationError::new(
                "update_signature_invalid",
                format!("更新包签名校验失败（0x{status:08X}），已拒绝安装。"),
            ));
        }
    };

    Ok(VerifiedUpdate {
        sha256: actual_hash,
        version: pe.version,
        signed,
    })
}

pub fn sha256_file(path: &Path) -> Result<String, UpdateVerificationError> {
    let mut file = File::open(path).map_err(|error| {
        UpdateVerificationError::new("update_io_failed", format!("无法读取更新包：{error}"))
    })?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer).map_err(|error| {
            UpdateVerificationError::new("update_io_failed", format!("读取更新包失败：{error}"))
        })?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

fn normalize_expected_sha256(value: &str) -> Result<String, UpdateVerificationError> {
    let normalized = value.trim().to_ascii_lowercase();
    if normalized.len() != HASH_HEX_LENGTH
        || !normalized.bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        return Err(UpdateVerificationError::new(
            "update_hash_invalid",
            "发布清单中的 SHA-256 格式无效。",
        ));
    }
    Ok(normalized)
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct PeInspection {
    version: Option<String>,
}

fn inspect_pe(path: &Path) -> Result<PeInspection, UpdateVerificationError> {
    let mut file = File::open(path).map_err(|error| {
        UpdateVerificationError::new("update_io_failed", format!("无法读取更新包：{error}"))
    })?;
    let mut header = [0_u8; DOS_HEADER_MIN_LENGTH];
    file.read_exact(&mut header).map_err(|_| {
        UpdateVerificationError::new("update_not_pe", "更新包不是有效的 Windows 可执行文件。")
    })?;
    if header[..2] != DOS_MZ_SIGNATURE {
        return Err(UpdateVerificationError::new(
            "update_not_pe",
            "更新包不是有效的 Windows 可执行文件。",
        ));
    }
    let pe_offset = u32::from_le_bytes(
        header[PE_OFFSET_LOCATION..PE_OFFSET_LOCATION + 4]
            .try_into()
            .expect("固定长度 PE 偏移"),
    ) as u64;
    file.seek(SeekFrom::Start(pe_offset))
        .map_err(|_| UpdateVerificationError::new("update_not_pe", "更新包的 PE 头偏移无效。"))?;

    let mut coff = [0_u8; 4 + PE_COFF_HEADER_LENGTH];
    file.read_exact(&mut coff)
        .map_err(|_| UpdateVerificationError::new("update_not_pe", "更新包的 PE 头不完整。"))?;
    if coff[..4] != PE_SIGNATURE {
        return Err(UpdateVerificationError::new(
            "update_not_pe",
            "更新包不是有效的 Windows 可执行文件。",
        ));
    }
    let mut optional_magic_bytes = [0_u8; 2];
    file.read_exact(&mut optional_magic_bytes)
        .map_err(|_| UpdateVerificationError::new("update_not_pe", "更新包缺少 PE 可选头。"))?;
    let optional_magic = u16::from_le_bytes(optional_magic_bytes);
    if optional_magic != PE64_OPTIONAL_MAGIC {
        return Err(UpdateVerificationError::new(
            "update_not_pe",
            "更新包不是 64 位 Windows 可执行文件。",
        ));
    }

    Ok(PeInspection {
        version: read_file_version(path),
    })
}

fn version_matches(expected: &str, actual: &str) -> bool {
    if expected.eq_ignore_ascii_case(actual) {
        return true;
    }
    let expected = numeric_version(expected);
    let actual = numeric_version(actual);
    let Some(expected) = expected else {
        return false;
    };
    let Some(actual) = actual else {
        return false;
    };
    expected
        .iter()
        .zip(actual.iter())
        .all(|(expected, actual)| expected == actual)
}

fn numeric_version(value: &str) -> Option<Vec<u32>> {
    let release = value.split(['-', '+']).next()?;
    let parts = release
        .split('.')
        .map(str::parse::<u32>)
        .collect::<Result<Vec<_>, _>>()
        .ok()?;
    (!parts.is_empty()).then_some(parts)
}

#[cfg(windows)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum AuthenticodeStatus {
    Trusted,
    Missing,
    Invalid(i32),
}

#[cfg(not(windows))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum AuthenticodeStatus {
    Missing,
}

#[cfg(windows)]
fn authenticode_status(path: &Path) -> Result<AuthenticodeStatus, UpdateVerificationError> {
    use std::{mem::size_of, os::windows::ffi::OsStrExt, ptr};
    use windows_sys::Win32::{
        Foundation::TRUST_E_NOSIGNATURE,
        Security::WinTrust::{
            WINTRUST_ACTION_GENERIC_VERIFY_V2, WINTRUST_DATA, WINTRUST_DATA_0, WINTRUST_FILE_INFO,
            WTD_CACHE_ONLY_URL_RETRIEVAL, WTD_CHOICE_FILE, WTD_REVOKE_NONE, WTD_STATEACTION_CLOSE,
            WTD_STATEACTION_VERIFY, WTD_UI_NONE, WinVerifyTrust,
        },
    };

    let mut wide_path = path.as_os_str().encode_wide().collect::<Vec<_>>();
    wide_path.push(0);
    let mut file_info = WINTRUST_FILE_INFO {
        cbStruct: size_of::<WINTRUST_FILE_INFO>() as u32,
        pcwszFilePath: wide_path.as_ptr(),
        hFile: ptr::null_mut(),
        pgKnownSubject: ptr::null_mut(),
    };
    let mut trust_data = WINTRUST_DATA {
        cbStruct: size_of::<WINTRUST_DATA>() as u32,
        dwUIChoice: WTD_UI_NONE,
        fdwRevocationChecks: WTD_REVOKE_NONE,
        dwUnionChoice: WTD_CHOICE_FILE,
        Anonymous: WINTRUST_DATA_0 {
            pFile: &mut file_info,
        },
        dwStateAction: WTD_STATEACTION_VERIFY,
        dwProvFlags: WTD_CACHE_ONLY_URL_RETRIEVAL,
        ..WINTRUST_DATA::default()
    };
    let mut action = WINTRUST_ACTION_GENERIC_VERIFY_V2;
    let status = unsafe {
        WinVerifyTrust(
            ptr::null_mut(),
            &mut action,
            (&mut trust_data as *mut WINTRUST_DATA).cast(),
        )
    };
    trust_data.dwStateAction = WTD_STATEACTION_CLOSE;
    let _ = unsafe {
        WinVerifyTrust(
            ptr::null_mut(),
            &mut action,
            (&mut trust_data as *mut WINTRUST_DATA).cast(),
        )
    };

    Ok(if status == 0 {
        AuthenticodeStatus::Trusted
    } else if status == TRUST_E_NOSIGNATURE {
        AuthenticodeStatus::Missing
    } else {
        AuthenticodeStatus::Invalid(status)
    })
}

#[cfg(not(windows))]
fn authenticode_status(_path: &Path) -> Result<AuthenticodeStatus, UpdateVerificationError> {
    Ok(AuthenticodeStatus::Missing)
}

#[cfg(windows)]
fn read_file_version(path: &Path) -> Option<String> {
    use std::{mem::size_of, os::windows::ffi::OsStrExt, ptr};
    use windows_sys::Win32::Storage::FileSystem::{
        GetFileVersionInfoSizeW, GetFileVersionInfoW, VS_FIXEDFILEINFO, VerQueryValueW,
    };

    let mut wide_path = path.as_os_str().encode_wide().collect::<Vec<_>>();
    wide_path.push(0);
    let mut ignored_handle = 0_u32;
    let size = unsafe { GetFileVersionInfoSizeW(wide_path.as_ptr(), &mut ignored_handle) };
    if size == 0 {
        return None;
    }
    let mut data = vec![0_u8; size as usize];
    if unsafe {
        GetFileVersionInfoW(
            wide_path.as_ptr(),
            0,
            size,
            data.as_mut_ptr().cast::<std::ffi::c_void>(),
        )
    } == 0
    {
        return None;
    }

    let mut subblock = [b'\\' as u16, 0];
    let mut value = ptr::null_mut::<std::ffi::c_void>();
    let mut value_length = 0_u32;
    if unsafe {
        VerQueryValueW(
            data.as_ptr().cast::<std::ffi::c_void>(),
            subblock.as_mut_ptr(),
            &mut value,
            &mut value_length,
        )
    } == 0
        || value_length < size_of::<VS_FIXEDFILEINFO>() as u32
    {
        return None;
    }
    let fixed = unsafe { ptr::read_unaligned(value.cast::<VS_FIXEDFILEINFO>()) };
    let version_ms = fixed.dwFileVersionMS;
    let version_ls = fixed.dwFileVersionLS;
    Some(format!(
        "{}.{}.{}.{}",
        (version_ms >> 16) & 0xffff,
        version_ms & 0xffff,
        (version_ls >> 16) & 0xffff,
        version_ls & 0xffff
    ))
}

#[cfg(not(windows))]
fn read_file_version(_path: &Path) -> Option<String> {
    None
}
