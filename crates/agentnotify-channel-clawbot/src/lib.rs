//! ClawBot 微信渠道的账号、密钥、登录、发送与入站边界。

mod account;
mod adapter;
mod client;
mod descriptor;
mod login;
mod qr;
mod state;

pub use account::{
    ClawBotAccount, bot_token_secret_ref, context_token_secret_ref, stable_account_id,
};
pub use adapter::ClawBotChannel;
pub use client::{ClawBotAuthTransport, ClawBotHttpClient, QrCodeResponse, QrStatusResponse};
pub use descriptor::{CLAWBOT_CHANNEL_ID, MAX_TEXT_BYTES, capabilities, descriptor};
pub use login::{ClawBotLoginAdapter, LoginDecision, LoginMachine};
pub use qr::qr_data_url;
pub use state::{
    ClawBotAccountState, ClawBotContext, ClawBotCredentials, ClawBotCursor, DEFAULT_BASE_URL,
};
