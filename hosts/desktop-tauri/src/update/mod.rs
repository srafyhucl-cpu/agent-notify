mod download;
mod error;
mod install;
mod release;
mod service;
mod verify;

pub use download::{
    DOWNLOAD_ATTEMPTS, HttpTextResponse, MAX_ARCHIVE_BYTES, MAX_CHECKSUMS_BYTES,
    ReqwestUpdateTransport, UpdateTransport, download_with_retry, ensure_checksum_matches,
    ensure_within_download_limit, expected_checksum, parse_checksum,
};
pub use error::UpdateError;
pub use install::{
    AppliedArchiveUpdate, INSTALLER_EARLY_EXIT_WINDOW, InstallerLaunchOutcome,
    InstallerLaunchRequest, InstallerLauncher, MAX_EXTRACTED_BYTES, StagedRelease,
    SystemInstallerLauncher, apply_staged_release, extract_archive, install_relative_path,
    installer_arguments, launch_installer, safe_entry_path, validate_staged_release,
    within_extraction_budget,
};
pub use release::{
    API_BASE_ENV, ArtifactKind, CHECKSUM_ASSET_NAME, DEFAULT_API_BASE_URL, DEFAULT_REPOSITORY,
    DEFAULT_WEB_BASE_URL, REPOSITORY_ENV, ReleaseInfo, archive_asset_name, check_latest_release,
    compare_versions, installer_asset_name, is_newer_version, normalize_version,
    parse_latest_release, release_tag_from_url,
};
pub use service::{InstallMode, InstallReport, UpdateChannel, UpdateConfig, UpdateService};
pub use verify::{
    DEFAULT_SIGNATURE_THUMBPRINT, SignatureRequirement, SignatureStatus, UpdateVerificationError,
    VerifiedUpdate, normalize_thumbprint, sha256_file, signature_policy, signer_thumbprint,
    trusted_thumbprints, verify_download, verify_executable, verify_signature,
};
