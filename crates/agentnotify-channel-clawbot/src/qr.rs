use agentnotify_channel_sdk::ChannelError;
use base64::{Engine as _, engine::general_purpose::STANDARD};
use qrcode::{QrCode, render::svg};

const QR_MIN_DIMENSION: u32 = 256;
pub(crate) const QR_DATA_URL_PREFIX: &str = concat!("data:", "image/", "svg+xml;base64,");

/// 二维码只编码为内存中的 SVG data URL，不经过文件系统。
pub fn qr_data_url(content: &str) -> Result<String, ChannelError> {
    let content = content.trim();
    if content.is_empty() {
        return Err(ChannelError::permanent(
            "clawbot_qr_empty",
            "ClawBot 登录二维码内容为空",
        ));
    }

    let code = QrCode::new(content.as_bytes()).map_err(|_| {
        ChannelError::permanent(
            "clawbot_qr_encode_failed",
            "ClawBot 登录二维码生成失败，请刷新后重试",
        )
    })?;
    let svg = code
        .render::<svg::Color>()
        .min_dimensions(QR_MIN_DIMENSION, QR_MIN_DIMENSION)
        .build();
    Ok(format!("{QR_DATA_URL_PREFIX}{}", STANDARD.encode(svg)))
}
