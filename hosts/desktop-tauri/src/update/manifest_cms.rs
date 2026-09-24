use std::{path::Path, time::SystemTime};

use super::error::UpdateError;

#[cfg(windows)]
use {
    super::verify::normalize_thumbprint,
    std::{fs, time::Duration},
};

#[cfg(windows)]
const SHA256_DIGEST_OID: &str = "2.16.840.1.101.3.4.2.1";
#[cfg(windows)]
const UNIX_EPOCH_FILETIME_TICKS: u64 = 116_444_736_000_000_000;

#[cfg(windows)]
pub(super) fn verify_detached_cms(
    manifest: &[u8],
    signature_path: &Path,
    now: SystemTime,
    trusted_thumbprints: &[String],
) -> Result<(), UpdateError> {
    use std::{ffi::CStr, mem::size_of, os::windows::ffi::OsStrExt, ptr};
    use windows_sys::Win32::Security::Cryptography::{
        CERT_CONTEXT, CERT_QUERY_CONTENT_FLAG_PKCS7_SIGNED, CERT_QUERY_FORMAT_FLAG_BINARY,
        CERT_QUERY_OBJECT_FILE, CMSG_SIGNER_COUNT_PARAM, CMSG_SIGNER_INFO, CMSG_SIGNER_INFO_PARAM,
        CRYPT_VERIFY_MESSAGE_PARA, CertCloseStore, CertFreeCertificateContext, CryptMsgClose,
        CryptMsgGetParam, CryptQueryObject, CryptVerifyDetachedMessageSignature,
        PKCS_7_ASN_ENCODING, X509_ASN_ENCODING,
    };

    let signature = fs::read(signature_path).map_err(|_| {
        UpdateError::new(
            "update_manifest_signature_invalid",
            "读取发布清单签名失败。",
        )
    })?;
    let mut wide_path = signature_path.as_os_str().encode_wide().collect::<Vec<_>>();
    wide_path.push(0);
    let mut encoding = 0_u32;
    let mut content_type = 0_u32;
    let mut format_type = 0_u32;
    let mut cert_store = ptr::null_mut();
    let mut message = ptr::null_mut();
    let queried = unsafe {
        CryptQueryObject(
            CERT_QUERY_OBJECT_FILE,
            wide_path.as_ptr().cast(),
            CERT_QUERY_CONTENT_FLAG_PKCS7_SIGNED,
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
    if queried == 0 || message.is_null() {
        return Err(cms_error("打开发布清单签名失败"));
    }

    let metadata_result = (|| {
        let mut signer_count_size = 0_u32;
        if unsafe {
            CryptMsgGetParam(
                message,
                CMSG_SIGNER_COUNT_PARAM,
                0,
                ptr::null_mut(),
                &mut signer_count_size,
            )
        } == 0
            || signer_count_size < size_of::<u32>() as u32
        {
            return Err(cms_error("读取发布清单签名者数量失败"));
        }
        let mut signer_count = 0_u32;
        let mut actual_count_size = signer_count_size;
        if unsafe {
            CryptMsgGetParam(
                message,
                CMSG_SIGNER_COUNT_PARAM,
                0,
                (&mut signer_count as *mut u32).cast(),
                &mut actual_count_size,
            )
        } == 0
            || actual_count_size < size_of::<u32>() as u32
            || signer_count != 1
        {
            return Err(UpdateError::new(
                "update_manifest_signature_invalid",
                "发布清单必须只有一个签名者。",
            ));
        }

        let mut signer_info_size = 0_u32;
        if unsafe {
            CryptMsgGetParam(
                message,
                CMSG_SIGNER_INFO_PARAM,
                0,
                ptr::null_mut(),
                &mut signer_info_size,
            )
        } == 0
            || signer_info_size < size_of::<CMSG_SIGNER_INFO>() as u32
        {
            return Err(UpdateError::new(
                "update_manifest_signature_invalid",
                "读取发布清单签名摘要算法失败。",
            ));
        }
        let word_size = size_of::<usize>();
        let word_count = (signer_info_size as usize).div_ceil(word_size);
        let mut signer_info_buffer = vec![0_usize; word_count];
        let mut actual_info_size = signer_info_size;
        if unsafe {
            CryptMsgGetParam(
                message,
                CMSG_SIGNER_INFO_PARAM,
                0,
                signer_info_buffer.as_mut_ptr().cast(),
                &mut actual_info_size,
            )
        } == 0
            || actual_info_size < size_of::<CMSG_SIGNER_INFO>() as u32
        {
            return Err(UpdateError::new(
                "update_manifest_signature_invalid",
                "读取发布清单签名摘要算法失败。",
            ));
        }
        let signer_info =
            unsafe { ptr::read_unaligned(signer_info_buffer.as_ptr().cast::<CMSG_SIGNER_INFO>()) };
        if signer_info.HashAlgorithm.pszObjId.is_null() {
            return Err(UpdateError::new(
                "update_manifest_signature_invalid",
                "读取发布清单签名摘要算法失败。",
            ));
        }
        let digest_oid = unsafe { CStr::from_ptr(signer_info.HashAlgorithm.pszObjId.cast()) };
        if digest_oid.to_bytes() != SHA256_DIGEST_OID.as_bytes() {
            return Err(UpdateError::new(
                "update_manifest_signature_invalid",
                "发布清单签名摘要算法不是 SHA-256。",
            ));
        }
        Ok(())
    })();
    unsafe {
        CryptMsgClose(message);
        CertCloseStore(cert_store, 0);
    }
    metadata_result?;

    let verify = CRYPT_VERIFY_MESSAGE_PARA {
        cbSize: size_of::<CRYPT_VERIFY_MESSAGE_PARA>() as u32,
        dwMsgAndCertEncodingType: X509_ASN_ENCODING | PKCS_7_ASN_ENCODING,
        ..Default::default()
    };
    let content = manifest.as_ptr();
    let content_size = manifest.len() as u32;
    let mut signer: *mut CERT_CONTEXT = ptr::null_mut();
    let verified = unsafe {
        CryptVerifyDetachedMessageSignature(
            &verify,
            0,
            signature.as_ptr(),
            signature.len() as u32,
            1,
            &content,
            &content_size,
            &mut signer,
        )
    };
    if verified == 0 || signer.is_null() {
        if !signer.is_null() {
            unsafe { CertFreeCertificateContext(signer) };
        }
        return Err(cms_error("验证发布清单签名失败"));
    }

    let result = verify_signer_certificate(signer, now, trusted_thumbprints);
    unsafe { CertFreeCertificateContext(signer) };
    result
}

#[cfg(windows)]
fn verify_signer_certificate(
    signer: *const windows_sys::Win32::Security::Cryptography::CERT_CONTEXT,
    now: SystemTime,
    trusted_thumbprints: &[String],
) -> Result<(), UpdateError> {
    use windows_sys::Win32::Security::Cryptography::{
        CERT_SHA1_HASH_PROP_ID, CertGetCertificateContextProperty,
    };

    let mut hash = [0_u8; 20];
    let mut hash_size = hash.len() as u32;
    if unsafe {
        CertGetCertificateContextProperty(
            signer,
            CERT_SHA1_HASH_PROP_ID,
            hash.as_mut_ptr().cast(),
            &mut hash_size,
        )
    } == 0
        || hash_size as usize != hash.len()
    {
        return Err(UpdateError::new(
            "update_manifest_signature_invalid",
            "无法读取发布清单签名者指纹。",
        ));
    }
    let thumbprint = hash
        .iter()
        .map(|byte| format!("{byte:02X}"))
        .collect::<String>();
    let trusted = trusted_thumbprints
        .iter()
        .map(|value| normalize_thumbprint(value))
        .filter(|value| !value.is_empty())
        .collect::<Vec<_>>();
    if !trusted.iter().any(|value| value == &thumbprint) {
        return Err(UpdateError::new(
            "update_manifest_signature_untrusted",
            "发布清单签名者不在信任列表中。",
        ));
    }

    let info = unsafe { (*signer).pCertInfo };
    if info.is_null() {
        return Err(UpdateError::new(
            "update_manifest_signature_invalid",
            "无法读取发布清单签名证书有效期。",
        ));
    }
    let not_before = filetime_to_system_time(unsafe { (*info).NotBefore });
    let not_after = filetime_to_system_time(unsafe { (*info).NotAfter });
    let (Some(not_before), Some(not_after)) = (not_before, not_after) else {
        return Err(UpdateError::new(
            "update_manifest_signature_invalid",
            "发布清单签名证书有效期无效。",
        ));
    };
    if now.duration_since(not_before).is_err() || not_after.duration_since(now).is_err() {
        return Err(UpdateError::new(
            "update_manifest_expired",
            "发布清单签名证书不在有效期内。",
        ));
    }
    Ok(())
}

#[cfg(windows)]
fn filetime_to_system_time(value: windows_sys::Win32::Foundation::FILETIME) -> Option<SystemTime> {
    let ticks = ((value.dwHighDateTime as u64) << 32) | value.dwLowDateTime as u64;
    if ticks < UNIX_EPOCH_FILETIME_TICKS {
        return None;
    }
    let delta = ticks - UNIX_EPOCH_FILETIME_TICKS;
    SystemTime::UNIX_EPOCH.checked_add(Duration::from_nanos(delta.saturating_mul(100)))
}

#[cfg(windows)]
fn cms_error(message: &str) -> UpdateError {
    use windows_sys::Win32::Foundation::GetLastError;
    let error = unsafe { GetLastError() };
    UpdateError::new(
        "update_manifest_signature_invalid",
        format!("{message}（Windows 错误码 {error}）。"),
    )
}

#[cfg(not(windows))]
pub(super) fn verify_detached_cms(
    _manifest: &[u8],
    _signature_path: &Path,
    _now: SystemTime,
    _trusted_thumbprints: &[String],
) -> Result<(), UpdateError> {
    Err(UpdateError::new(
        "update_manifest_signature_unsupported",
        "当前平台不支持验证发布清单签名。",
    ))
}
