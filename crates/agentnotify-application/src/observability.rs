use sha2::{Digest, Sha256};

/// 日志只记录稳定标识的短哈希，避免正文、会话标识和密钥进入 span。
pub(crate) fn hash_identifier(value: &str) -> String {
    let digest = Sha256::digest(value.as_bytes());
    let mut output = String::with_capacity(16);
    for byte in &digest[..8] {
        use std::fmt::Write as _;
        write!(output, "{byte:02x}").expect("写入 String 不会失败");
    }
    output
}
