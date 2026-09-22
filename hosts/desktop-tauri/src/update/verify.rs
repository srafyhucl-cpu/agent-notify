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

/// 内置信任指纹，与 Go 版 `internal/update/signature.go` 的 defaultSignatureThumbprint 一致：
/// 客户端只接受由该证书签名的更新包，轮换证书时必须先更新这里并发版。
pub const DEFAULT_SIGNATURE_THUMBPRINT: &str = "EDF9E283DF2407B318E65D59BB430FD546509ACD";

/// 覆盖信任指纹的环境变量（与 Go 版同名，逗号/分号分隔）；留空时使用内置指纹。
const SIGNATURE_THUMBPRINT_ENV: &str = "AGENT_NOTIFY_SIGNATURE_THUMBPRINT";

/// 更新包签名策略。正式通道必须使用 `Required`，预览包可以显式使用 `Optional`。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SignatureRequirement {
    Required,
    Optional,
}

/// Authenticode 检查结果，取值与 Go 版 `Get-AuthenticodeSignature` 的状态对齐。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SignatureStatus {
    Valid,
    NotSigned,
    HashMismatch,
    NotTrusted,
    UnknownError,
}

impl SignatureStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Valid => "Valid",
            Self::NotSigned => "NotSigned",
            Self::HashMismatch => "HashMismatch",
            Self::NotTrusted => "NotTrusted",
            Self::UnknownError => "UnknownError",
        }
    }
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
/// 正式通道（`Required`）在签名有效的前提下还要求签名者指纹命中信任列表。
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

    let (version, signed) = verify_pe_and_signature(path, signature_requirement, expected_version)?;
    Ok(VerifiedUpdate {
        sha256: actual_hash,
        version,
        signed,
    })
}

/// 校验一个已解包的可执行文件（ZIP 回退路径用），同样检查 PE、文件版本和 Authenticode。
pub fn verify_executable(
    path: &Path,
    signature_requirement: SignatureRequirement,
    expected_version: Option<&str>,
) -> Result<VerifiedUpdate, UpdateVerificationError> {
    let sha256 = sha256_file(path)?;
    let (version, signed) = verify_pe_and_signature(path, signature_requirement, expected_version)?;
    Ok(VerifiedUpdate {
        sha256,
        version,
        signed,
    })
}

/// 单独校验 Authenticode 签名，返回是否带签名（预览通道允许未签名）。
pub fn verify_signature(
    path: &Path,
    signature_requirement: SignatureRequirement,
) -> Result<bool, UpdateVerificationError> {
    let (status, thumbprint) = inspect_authenticode(path)?;
    signature_policy(
        status,
        &thumbprint,
        signature_requirement,
        &trusted_thumbprints(),
    )
}

/// 当前信任的签名指纹：默认是内置指纹，可用 `AGENT_NOTIFY_SIGNATURE_THUMBPRINT` 覆盖（与 Go 版一致）。
pub fn trusted_thumbprints() -> Vec<String> {
    let raw = std::env::var(SIGNATURE_THUMBPRINT_ENV).unwrap_or_default();
    if !raw.trim().is_empty() {
        return raw
            .split([',', ';'])
            .map(normalize_thumbprint)
            .filter(|value| !value.is_empty())
            .collect();
    }
    let pinned = normalize_thumbprint(DEFAULT_SIGNATURE_THUMBPRINT);
    if pinned.is_empty() {
        Vec::new()
    } else {
        vec![pinned]
    }
}

/// 归一化指纹：去掉空白与冒号并转大写，比较时不受书写格式影响。
pub fn normalize_thumbprint(value: &str) -> String {
    value
        .chars()
        .filter(|character| !character.is_whitespace())
        .collect::<String>()
        .replace(':', "")
        .to_ascii_uppercase()
}

/// 签名放行规则的纯函数，与 Go 版 `signaturePolicy` 语义一致，便于测试。
/// 指纹只在正式通道（`Required`）生效：预览通道显式放宽，允许未签名包并如实返回 signed=false。
pub fn signature_policy(
    status: SignatureStatus,
    thumbprint: &str,
    requirement: SignatureRequirement,
    pinned: &[String],
) -> Result<bool, UpdateVerificationError> {
    let pinned: Vec<String> = pinned
        .iter()
        .map(|value| normalize_thumbprint(value))
        .filter(|value| !value.is_empty())
        .collect();

    if requirement == SignatureRequirement::Required && !pinned.is_empty() {
        // 配置了指纹后，指纹本身就是信任锚：自签名/根不受信（UnknownError、NotTrusted）
        // 只要指纹匹配就放行，但篡改类状态（HashMismatch、NotSigned 等）一律拒绝。
        if !acceptable_when_pinned(status) {
            if status == SignatureStatus::NotSigned {
                return Err(UpdateVerificationError::new(
                    "update_signature_missing",
                    "更新包未签名，已按正式通道策略拒绝安装。",
                ));
            }
            return Err(UpdateVerificationError::new(
                "update_signature_abnormal",
                format!("更新包签名状态异常（{}），拒绝安装。", status.as_str()),
            ));
        }
        let actual = normalize_thumbprint(thumbprint);
        if actual.is_empty() || !pinned.contains(&actual) {
            return Err(UpdateVerificationError::new(
                "update_signature_untrusted",
                format!(
                    "更新包签名者不匹配：实际 {}，不在信任列表中，拒绝安装。",
                    display_thumbprint(thumbprint)
                ),
            ));
        }
        return Ok(true);
    }

    match status {
        SignatureStatus::Valid => Ok(true),
        SignatureStatus::HashMismatch | SignatureStatus::NotTrusted => {
            // 有签名但校验失败：最明确的篡改/不可信信号，任何策略下都拒绝。
            Err(UpdateVerificationError::new(
                "update_signature_invalid",
                format!("更新包签名无效（{}），拒绝安装。", status.as_str()),
            ))
        }
        SignatureStatus::NotSigned | SignatureStatus::UnknownError => {
            if requirement == SignatureRequirement::Required {
                Err(UpdateVerificationError::new(
                    "update_signature_missing",
                    "更新包未签名，已按正式通道策略拒绝安装。",
                ))
            } else {
                Ok(false)
            }
        }
    }
}

fn verify_pe_and_signature(
    path: &Path,
    signature_requirement: SignatureRequirement,
    expected_version: Option<&str>,
) -> Result<(Option<String>, bool), UpdateVerificationError> {
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

    let signed = verify_signature(path, signature_requirement)?;
    Ok((pe.version, signed))
}

/// acceptableWhenPinned：签名存在且未被判为篡改的状态；自签名证书因根不受信会得到
/// UnknownError/NotTrusted，此时以指纹作为信任锚。
fn acceptable_when_pinned(status: SignatureStatus) -> bool {
    matches!(
        status,
        SignatureStatus::Valid | SignatureStatus::UnknownError | SignatureStatus::NotTrusted
    )
}

fn display_thumbprint(value: &str) -> String {
    let normalized = normalize_thumbprint(value);
    if normalized.is_empty() {
        "无签名者".to_owned()
    } else {
        normalized
    }
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
fn inspect_authenticode(path: &Path) -> Result<(SignatureStatus, String), UpdateVerificationError> {
    let status = wintrust_status(path)?;
    let thumbprint = if status == SignatureStatus::NotSigned {
        String::new()
    } else {
        signer_thumbprint(path).unwrap_or_default()
    };
    Ok((status, thumbprint))
}

#[cfg(not(windows))]
fn inspect_authenticode(
    _path: &Path,
) -> Result<(SignatureStatus, String), UpdateVerificationError> {
    Ok((SignatureStatus::NotSigned, String::new()))
}

#[cfg(windows)]
fn wintrust_status(path: &Path) -> Result<SignatureStatus, UpdateVerificationError> {
    use std::{mem::size_of, os::windows::ffi::OsStrExt, ptr};
    use windows_sys::Win32::Security::WinTrust::{
        WINTRUST_ACTION_GENERIC_VERIFY_V2, WINTRUST_DATA, WINTRUST_DATA_0, WINTRUST_FILE_INFO,
        WTD_CACHE_ONLY_URL_RETRIEVAL, WTD_CHOICE_FILE, WTD_REVOKE_NONE, WTD_STATEACTION_CLOSE,
        WTD_STATEACTION_VERIFY, WTD_UI_NONE, WinVerifyTrust,
    };

    // Trust 提供程序返回的稳定错误码，与 Get-AuthenticodeSignature 的状态对齐。
    const TRUST_E_NOSIGNATURE: i32 = 0x800B0100u32 as i32;
    const TRUST_E_BAD_DIGEST: i32 = 0x80096010u32 as i32;
    const TRUST_E_EXPLICIT_DISTRUST: i32 = 0x800B0111u32 as i32;
    const TRUST_E_SUBJECT_NOT_TRUSTED: i32 = 0x800B0004u32 as i32;
    const TRUST_E_CERT_SIGNATURE: i32 = 0x80096004u32 as i32;
    const CERT_E_UNTRUSTEDROOT: i32 = 0x800B0109u32 as i32;
    const CERT_E_CHAINING: i32 = 0x800B010Au32 as i32;
    const CERT_E_EXPIRED: i32 = 0x800B0101u32 as i32;
    const CERT_E_REVOKED: i32 = 0x800B010Cu32 as i32;
    const CRYPT_E_SECURITY_SETTINGS: i32 = 0x80092026u32 as i32;

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
        SignatureStatus::Valid
    } else if status == TRUST_E_NOSIGNATURE {
        SignatureStatus::NotSigned
    } else if status == TRUST_E_BAD_DIGEST {
        SignatureStatus::HashMismatch
    } else if matches!(
        status,
        TRUST_E_EXPLICIT_DISTRUST
            | TRUST_E_SUBJECT_NOT_TRUSTED
            | TRUST_E_CERT_SIGNATURE
            | CERT_E_UNTRUSTEDROOT
            | CERT_E_CHAINING
            | CERT_E_EXPIRED
            | CERT_E_REVOKED
            | CRYPT_E_SECURITY_SETTINGS
    ) {
        SignatureStatus::NotTrusted
    } else {
        SignatureStatus::UnknownError
    })
}

#[cfg(not(windows))]
fn wintrust_status(_path: &Path) -> Result<SignatureStatus, UpdateVerificationError> {
    Ok(SignatureStatus::NotSigned)
}

/// 读取 Authenticode 签名者证书的 SHA-1 指纹（大写十六进制），失败时返回 None。
#[cfg(windows)]
pub fn signer_thumbprint(path: &Path) -> Option<String> {
    use std::{mem::size_of, os::windows::ffi::OsStrExt, ptr};
    use windows_sys::Win32::Security::Cryptography::{
        CERT_CONTEXT, CERT_QUERY_CONTENT_FLAG_PKCS7_SIGNED_EMBED, CERT_QUERY_FORMAT_FLAG_BINARY,
        CERT_QUERY_OBJECT_FILE, CERT_SHA1_HASH_PROP_ID, CMSG_SIGNER_ONLY_FLAG, CertCloseStore,
        CertFreeCertificateContext, CertGetCertificateContextProperty, CryptMsgClose,
        CryptMsgGetAndVerifySigner, CryptQueryObject,
    };

    let mut wide_path = path.as_os_str().encode_wide().collect::<Vec<_>>();
    wide_path.push(0);

    let mut encoding = 0u32;
    let mut content_type = 0u32;
    let mut format_type = 0u32;
    let mut cert_store = ptr::null_mut();
    let mut message = ptr::null_mut();
    let queried = unsafe {
        CryptQueryObject(
            CERT_QUERY_OBJECT_FILE,
            wide_path.as_ptr().cast(),
            CERT_QUERY_CONTENT_FLAG_PKCS7_SIGNED_EMBED,
            CERT_QUERY_FORMAT_FLAG_BINARY,
            0,
            &mut encoding,
            &mut content_type,
            &mut format_type,
            &mut cert_store,
            &mut message,
            ptr::null_mut(),
        )
    };
    if queried == 0 {
        return None;
    }

    let mut signer: *mut CERT_CONTEXT = ptr::null_mut();
    let signer_ok = unsafe {
        CryptMsgGetAndVerifySigner(
            message,
            0,
            ptr::null(),
            CMSG_SIGNER_ONLY_FLAG,
            &mut signer,
            ptr::null_mut(),
        )
    };

    let mut thumbprint = None;
    if signer_ok != 0 && !signer.is_null() {
        let mut hash = [0u8; size_of::<[u32; 5]>()];
        let mut hash_len = hash.len() as u32;
        let hashed = unsafe {
            CertGetCertificateContextProperty(
                signer,
                CERT_SHA1_HASH_PROP_ID,
                hash.as_mut_ptr().cast(),
                &mut hash_len,
            )
        };
        if hashed != 0 && hash_len as usize == hash.len() {
            thumbprint = Some(
                hash.iter()
                    .map(|byte| format!("{byte:02X}"))
                    .collect::<String>(),
            );
        }
        unsafe { CertFreeCertificateContext(signer) };
    }

    unsafe {
        CryptMsgClose(message);
        CertCloseStore(cert_store, 0);
    }
    thumbprint
}

#[cfg(not(windows))]
pub fn signer_thumbprint(_path: &Path) -> Option<String> {
    None
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
