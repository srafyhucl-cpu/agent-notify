//! 测试用临时根目录：环境变量优先，其次本地 D 盘约定，最后系统临时目录。

use std::path::{Path, PathBuf};

/// 显式覆盖测试临时根的环境变量；优先级最高，也用于在没有 D 盘的机器上验证回退路径。
pub const TEST_TEMP_DIR_ENV: &str = "AGENTNOTIFY_TEST_TEMP_DIR";

/// 本地约定：临时文件留在 D 盘，仅在目录真实存在时使用。
const PREFERRED_TEMP_ROOT: &str = r"D:\Temp";

/// 选择顺序：`AGENTNOTIFY_TEST_TEMP_DIR` → `D:\Temp`（存在时）→ 系统临时目录。
///
/// 返回前保证目录已经存在：`tempfile::Builder::tempdir_in` 要求父目录存在。
pub fn test_temp_root() -> PathBuf {
    let override_dir = std::env::var_os(TEST_TEMP_DIR_ENV).map(PathBuf::from);
    let root = choose_temp_root(
        override_dir,
        Path::new(PREFERRED_TEMP_ROOT).is_dir(),
        std::env::temp_dir(),
    );
    ensure_directory(root)
}

/// 纯选择逻辑：文件系统判定由调用方注入，单元测试不依赖本机是否真的有 D 盘。
fn choose_temp_root(
    override_dir: Option<PathBuf>,
    preferred_available: bool,
    system_temp: PathBuf,
) -> PathBuf {
    if let Some(dir) = override_dir.filter(|dir| !dir.as_os_str().is_empty()) {
        return dir;
    }
    if preferred_available {
        return PathBuf::from(PREFERRED_TEMP_ROOT);
    }
    system_temp
}

/// 保证目录存在；创建失败必须直接暴露，不能悄悄换一个目录继续跑。
fn ensure_directory(dir: PathBuf) -> PathBuf {
    std::fs::create_dir_all(&dir)
        .unwrap_or_else(|error| panic!("测试临时根目录不可创建 {}：{error}", dir.display()));
    dir
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn environment_override_takes_priority() {
        let override_dir = PathBuf::from(r"C:\Temp\agentnotify-test-temp");
        assert_eq!(
            choose_temp_root(
                Some(override_dir.clone()),
                true,
                PathBuf::from(r"C:\Windows\Temp"),
            ),
            override_dir
        );
    }

    #[test]
    fn preferred_root_is_used_when_the_directory_exists() {
        assert_eq!(
            choose_temp_root(None, true, PathBuf::from(r"C:\Windows\Temp")),
            PathBuf::from(PREFERRED_TEMP_ROOT)
        );
    }

    #[test]
    fn system_temp_is_the_last_resort() {
        let system_temp = PathBuf::from(r"C:\Windows\Temp");
        assert_eq!(
            choose_temp_root(None, false, system_temp.clone()),
            system_temp
        );
    }

    #[test]
    fn blank_override_falls_back_instead_of_returning_an_empty_path() {
        let system_temp = PathBuf::from(r"C:\Windows\Temp");
        assert_eq!(
            choose_temp_root(Some(PathBuf::new()), false, system_temp.clone()),
            system_temp
        );
    }

    /// 真实调用只断言目录存在，不依赖本机是否有 D 盘。
    #[test]
    fn test_temp_root_returns_an_existing_directory() {
        let root = test_temp_root();
        assert!(root.is_dir(), "{}", root.display());
    }
}
