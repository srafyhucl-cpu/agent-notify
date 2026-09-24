use std::{
    collections::BTreeMap,
    fs,
    path::{Component, Path, PathBuf},
    sync::OnceLock,
    time::SystemTime,
};

use serde::Deserialize;

use super::{
    error::UpdateError,
    manifest_cms::verify_detached_cms,
    verify::{SignatureRequirement, sha256_file},
};

/// 发布清单的跨语言合同；Rust 与 PowerShell 门禁必须读取同一份内容。
const RELEASE_MANIFEST_CONTRACT: &str =
    include_str!("../../../../config/release-manifest-contract.json");
const SHA256_HEX_LENGTH: usize = 64;

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ReleaseManifestContract {
    schema_version: u32,
    product: String,
    manifest_file_name: String,
    signature_file_name: String,
    hash_algorithm: String,
    max_manifest_bytes: u64,
    max_signature_bytes: u64,
    max_file_entries: usize,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ReleaseManifest {
    schema_version: u32,
    product: String,
    version: String,
    hash_algorithm: String,
    files: Vec<ManifestFile>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ManifestFile {
    pub path: String,
    pub size: u64,
    pub sha256: String,
}

/// 清单验证结果；Legacy 只在 Beta 且两个控制文件都不存在时产生。
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum VerifiedReleaseManifest {
    Legacy,
    Signed { files: Vec<ManifestFile> },
}

fn release_manifest_contract() -> Result<&'static ReleaseManifestContract, UpdateError> {
    static CONTRACT: OnceLock<Result<ReleaseManifestContract, String>> = OnceLock::new();
    match CONTRACT.get_or_init(|| {
        let contract: ReleaseManifestContract = serde_json::from_str(RELEASE_MANIFEST_CONTRACT)
            .map_err(|error| format!("发布清单合同无效：{error}"))?;
        let invalid_name =
            |name: &str| name.is_empty() || name.contains('/') || name.contains('\\');
        if contract.schema_version != 1
            || contract.product.is_empty()
            || contract.hash_algorithm != "sha256"
            || contract.max_manifest_bytes == 0
            || contract.max_signature_bytes == 0
            || contract.max_file_entries == 0
            || contract.manifest_file_name == contract.signature_file_name
            || invalid_name(&contract.manifest_file_name)
            || invalid_name(&contract.signature_file_name)
        {
            return Err("发布清单合同字段超出支持范围".to_owned());
        }
        Ok(contract)
    }) {
        Ok(contract) => Ok(contract),
        Err(message) => Err(UpdateError::new("update_manifest_invalid", message)),
    }
}

/// 验证 ZIP 发布根目录中的清单；测试通过 `now` 和信任列表注入避免修改全局环境。
pub fn verify_release_manifest_at(
    root: &Path,
    expected_version: &str,
    requirement: SignatureRequirement,
    now: SystemTime,
    trusted_thumbprints: &[String],
) -> Result<VerifiedReleaseManifest, UpdateError> {
    let contract = release_manifest_contract()?;
    let root_metadata = fs::symlink_metadata(root)
        .map_err(|_| UpdateError::new("update_manifest_invalid", "读取更新包目录失败。"))?;
    if root_metadata.file_type().is_symlink() || !root_metadata.is_dir() {
        return Err(UpdateError::new(
            "update_manifest_invalid",
            "更新包根目录无效。",
        ));
    }
    let manifest_path = root.join(&contract.manifest_file_name);
    let signature_path = root.join(&contract.signature_file_name);
    let manifest_state = control_file_state(&manifest_path)?;
    let signature_state = control_file_state(&signature_path)?;

    if manifest_state == ControlFileState::Missing && signature_state == ControlFileState::Missing {
        if requirement == SignatureRequirement::Optional {
            return Ok(VerifiedReleaseManifest::Legacy);
        }
        return Err(UpdateError::new(
            "update_manifest_missing",
            "正式更新包缺少发布清单，已拒绝安装。",
        ));
    }
    if manifest_state == ControlFileState::Missing {
        return Err(UpdateError::new(
            "update_manifest_missing",
            "正式更新包缺少发布清单，已拒绝安装。",
        ));
    }
    if signature_state == ControlFileState::Missing {
        return Err(UpdateError::new(
            "update_manifest_signature_missing",
            "发布清单缺少签名，已拒绝安装。",
        ));
    }

    let manifest_bytes = read_limited(
        &manifest_path,
        contract.max_manifest_bytes,
        "update_manifest_invalid",
        "读取发布清单失败",
    )?;
    read_limited(
        &signature_path,
        contract.max_signature_bytes,
        "update_manifest_signature_invalid",
        "读取发布清单签名失败",
    )?;

    verify_detached_cms(&manifest_bytes, &signature_path, now, trusted_thumbprints)?;
    let manifest: ReleaseManifest = serde_json::from_slice(&manifest_bytes).map_err(|_| {
        UpdateError::new("update_manifest_invalid", "发布清单格式无效，已拒绝安装。")
    })?;
    validate_manifest_header(&manifest, contract, expected_version)?;

    let actual_files = collect_actual_files(root, contract)?;
    let mut remaining: BTreeMap<String, PathBuf> = actual_files.into_iter().collect();
    for file in &manifest.files {
        let Some(path) = remaining.remove(&file.path) else {
            return Err(UpdateError::new(
                "update_manifest_file_missing",
                "更新包缺少发布清单声明的文件，已拒绝安装。",
            ));
        };
        let metadata = fs::metadata(&path).map_err(|_| {
            UpdateError::new("update_manifest_hash_mismatch", "读取发布清单文件失败。")
        })?;
        if metadata.len() != file.size {
            return Err(UpdateError::new(
                "update_manifest_hash_mismatch",
                "更新包文件大小与发布清单不一致，已拒绝安装。",
            ));
        }
        let actual_hash = sha256_file(&path)?;
        if actual_hash != file.sha256 {
            return Err(UpdateError::new(
                "update_manifest_hash_mismatch",
                "更新包文件校验值与发布清单不一致，已拒绝安装。",
            ));
        }
    }
    if let Some(extra) = remaining.keys().next() {
        return Err(UpdateError::new(
            "update_manifest_file_extra",
            format!("更新包包含发布清单未声明的文件：{extra}。"),
        ));
    }

    Ok(VerifiedReleaseManifest::Signed {
        files: manifest.files,
    })
}

fn validate_manifest_header(
    manifest: &ReleaseManifest,
    contract: &ReleaseManifestContract,
    expected_version: &str,
) -> Result<(), UpdateError> {
    if manifest.schema_version != contract.schema_version
        || manifest.product != contract.product
        || manifest.version != expected_version
        || manifest.hash_algorithm != contract.hash_algorithm
    {
        return Err(UpdateError::new(
            "update_manifest_invalid",
            "发布清单的版本、产品或哈希算法不匹配。",
        ));
    }
    if manifest.files.is_empty() || manifest.files.len() > contract.max_file_entries {
        return Err(UpdateError::new(
            "update_manifest_invalid",
            "发布清单文件数量无效。",
        ));
    }

    let mut previous: Option<&str> = None;
    for file in &manifest.files {
        validate_manifest_path(&file.path)?;
        if file.sha256.len() != SHA256_HEX_LENGTH
            || !file.sha256.bytes().all(|byte| byte.is_ascii_hexdigit())
            || file.sha256.bytes().any(|byte| byte.is_ascii_uppercase())
        {
            return Err(UpdateError::new(
                "update_manifest_invalid",
                "发布清单中的 SHA-256 格式无效。",
            ));
        }
        if let Some(previous) = previous {
            if file.path.as_str() <= previous {
                return Err(UpdateError::new(
                    "update_manifest_invalid",
                    "发布清单文件路径未按固定顺序排列或存在重复。",
                ));
            }
            if file.path.eq_ignore_ascii_case(previous) {
                return Err(UpdateError::new(
                    "update_manifest_invalid",
                    "发布清单包含大小写冲突的文件路径。",
                ));
            }
        }
        previous = Some(&file.path);
    }
    Ok(())
}

fn validate_manifest_path(value: &str) -> Result<(), UpdateError> {
    if value.is_empty()
        || value.contains('\\')
        || value.contains('\0')
        || value.starts_with('/')
        || value.ends_with('/')
        || value.contains("//")
        || value.chars().any(char::is_control)
    {
        return Err(UpdateError::new(
            "update_manifest_invalid",
            "发布清单包含不安全的文件路径。",
        ));
    }
    let mut components = Path::new(value).components();
    let Some(Component::Normal(first)) = components.next() else {
        return Err(UpdateError::new(
            "update_manifest_invalid",
            "发布清单包含不安全的文件路径。",
        ));
    };
    if first.is_empty() {
        return Err(UpdateError::new(
            "update_manifest_invalid",
            "发布清单包含不安全的文件路径。",
        ));
    }
    for component in components {
        if !matches!(component, Component::Normal(_)) {
            return Err(UpdateError::new(
                "update_manifest_invalid",
                "发布清单包含不安全的文件路径。",
            ));
        }
    }
    Ok(())
}

fn collect_actual_files(
    root: &Path,
    contract: &ReleaseManifestContract,
) -> Result<Vec<(String, PathBuf)>, UpdateError> {
    fn visit(
        root: &Path,
        current: &Path,
        contract: &ReleaseManifestContract,
        output: &mut Vec<(String, PathBuf)>,
    ) -> Result<(), UpdateError> {
        let entries = fs::read_dir(current)
            .map_err(|_| UpdateError::new("update_manifest_file_extra", "读取更新包文件失败。"))?;
        for entry in entries {
            let entry = entry.map_err(|_| {
                UpdateError::new("update_manifest_file_extra", "读取更新包文件失败。")
            })?;
            let path = entry.path();
            let metadata = fs::symlink_metadata(&path).map_err(|_| {
                UpdateError::new("update_manifest_file_extra", "读取更新包文件失败。")
            })?;
            if metadata.file_type().is_symlink() {
                return Err(UpdateError::new(
                    "update_manifest_file_extra",
                    "更新包包含不支持的符号链接。",
                ));
            }
            if metadata.is_dir() {
                visit(root, &path, contract, output)?;
                continue;
            }
            if !metadata.is_file() {
                return Err(UpdateError::new(
                    "update_manifest_file_extra",
                    "更新包包含不支持的特殊文件。",
                ));
            }
            let relative = path.strip_prefix(root).map_err(|_| {
                UpdateError::new("update_manifest_file_extra", "更新包文件路径无效。")
            })?;
            let relative = relative.to_string_lossy().replace('\\', "/");
            if relative == contract.manifest_file_name || relative == contract.signature_file_name {
                continue;
            }
            output.push((relative, path));
        }
        Ok(())
    }

    let mut output = Vec::new();
    visit(root, root, contract, &mut output)?;
    if output.len() > contract.max_file_entries {
        return Err(UpdateError::new(
            "update_manifest_file_extra",
            "更新包文件数量超过发布清单上限。",
        ));
    }
    output.sort_by(|left, right| left.0.cmp(&right.0));
    Ok(output)
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum ControlFileState {
    Missing,
    Present,
}

fn control_file_state(path: &Path) -> Result<ControlFileState, UpdateError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => Err(
            UpdateError::new("update_manifest_invalid", "发布清单控制文件无效。"),
        ),
        Ok(_) => Ok(ControlFileState::Present),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(ControlFileState::Missing),
        Err(_) => Err(UpdateError::new(
            "update_manifest_invalid",
            "读取发布清单控制文件失败。",
        )),
    }
}

fn read_limited(
    path: &Path,
    limit: u64,
    code: &'static str,
    message: &'static str,
) -> Result<Vec<u8>, UpdateError> {
    let metadata =
        fs::metadata(path).map_err(|_| UpdateError::new(code, format!("{message}。")))?;
    if metadata.len() > limit {
        return Err(UpdateError::new(
            "update_manifest_invalid",
            format!("{message}：文件超过大小上限。"),
        ));
    }
    let bytes = fs::read(path).map_err(|_| UpdateError::new(code, format!("{message}。")))?;
    if bytes.len() as u64 > limit {
        return Err(UpdateError::new(
            "update_manifest_invalid",
            format!("{message}：文件超过大小上限。"),
        ));
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn contract_is_valid_and_contains_no_unix_path() {
        let contract = release_manifest_contract().expect("内置发布清单合同必须有效");
        assert_eq!(contract.schema_version, 1);
        assert_eq!(contract.hash_algorithm, "sha256");
        assert!(contract.max_manifest_bytes > 0);
    }

    #[test]
    fn path_validation_rejects_traversal_and_backslashes() {
        for path in ["../evil", "a/../b", "/absolute", "a\\b", "a//b", "a/"] {
            assert!(
                validate_manifest_path(path).is_err(),
                "must reject {path:?}"
            );
        }
        assert!(validate_manifest_path("plugin/agent-notify.ts").is_ok());
    }

    fn valid_manifest(contract: &ReleaseManifestContract) -> ReleaseManifest {
        ReleaseManifest {
            schema_version: contract.schema_version,
            product: contract.product.clone(),
            version: "2.0.7".to_owned(),
            hash_algorithm: contract.hash_algorithm.clone(),
            files: vec![ManifestFile {
                path: "VERSION".to_owned(),
                size: 5,
                sha256: "0".repeat(SHA256_HEX_LENGTH),
            }],
        }
    }

    #[test]
    fn strict_json_and_manifest_headers_reject_ambiguity() {
        let contract = release_manifest_contract().expect("内置发布清单合同必须有效");
        let unknown = br#"{
            "schemaVersion":1,
            "product":"AgentNotify",
            "version":"2.0.7",
            "hashAlgorithm":"sha256",
            "files":[],
            "unexpected":true
        }"#;
        assert!(serde_json::from_slice::<ReleaseManifest>(unknown).is_err());

        let mut manifest = valid_manifest(contract);
        manifest.version = "2.0.8".to_owned();
        assert_eq!(
            validate_manifest_header(&manifest, contract, "2.0.7")
                .expect_err("版本不匹配必须拒绝")
                .code(),
            "update_manifest_invalid"
        );

        manifest = valid_manifest(contract);
        manifest.files.push(manifest.files[0].clone());
        assert!(validate_manifest_header(&manifest, contract, "2.0.7").is_err());

        manifest = valid_manifest(contract);
        manifest.files[0].sha256 = "A".repeat(SHA256_HEX_LENGTH);
        assert!(validate_manifest_header(&manifest, contract, "2.0.7").is_err());
    }
}
