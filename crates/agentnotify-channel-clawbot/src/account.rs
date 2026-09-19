use std::fmt::Write as _;

use agentnotify_channel_sdk::{ChannelAccount, ChannelError, SecretRef};
use agentnotify_domain::{ChannelAccountId, ChannelId, Timestamp};
use sha2::{Digest, Sha256};

use crate::{
    descriptor::CLAWBOT_CHANNEL_ID,
    state::{ClawBotAccountState, ClawBotCursor},
};

const ACCOUNT_ID_HASH_HEX_LENGTH: usize = 16;
const PLATFORM_ID_HINT_CHARACTERS: usize = 6;

/// ClawBot 账号的稳定标识与脱敏元数据边界。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ClawBotAccount {
    channel: ChannelAccount,
    state: ClawBotAccountState,
    cursor: ClawBotCursor,
}

impl ClawBotAccount {
    pub fn from_platform_ids(
        bot_id: impl AsRef<str>,
        user_id: impl AsRef<str>,
    ) -> Result<Self, ChannelError> {
        Self::from_platform_ids_at(bot_id, user_id, Timestamp::now_utc())
    }

    pub fn from_platform_ids_at(
        bot_id: impl AsRef<str>,
        user_id: impl AsRef<str>,
        created_at: Timestamp,
    ) -> Result<Self, ChannelError> {
        let bot_id = normalize_platform_id("bot_id", bot_id.as_ref())?;
        let user_id = normalize_platform_id("user_id", user_id.as_ref())?;
        let id = stable_account_id(&bot_id, &user_id)?;
        let state = ClawBotAccountState::new(tail_hint(&bot_id), tail_hint(&user_id));
        let cursor = ClawBotCursor::default();
        let mut channel = ChannelAccount::new(
            id,
            ChannelId::new(CLAWBOT_CHANNEL_ID).expect("ClawBot 渠道 ID 是固定有效值"),
            display_name(&state),
            created_at,
        );
        sync_serialized_state(&mut channel, &state, &cursor)?;
        channel.secret_ref = Some(bot_token_secret_ref(&channel.id)?);
        Ok(Self {
            channel,
            state,
            cursor,
        })
    }

    pub fn from_channel_account(account: ChannelAccount) -> Result<Self, ChannelError> {
        if account.channel_id.as_str() != CLAWBOT_CHANNEL_ID {
            return Err(ChannelError::permanent(
                "clawbot_account_channel_mismatch",
                "该账号不属于 ClawBot 渠道",
            ));
        }
        let state = serde_json::from_value::<ClawBotAccountState>(account.config.clone()).map_err(
            |_| {
                ChannelError::permanent(
                    "clawbot_account_state_invalid",
                    "ClawBot 账号状态损坏，请重新登录",
                )
            },
        )?;
        let cursor =
            serde_json::from_value::<ClawBotCursor>(account.cursor.clone()).map_err(|_| {
                ChannelError::permanent(
                    "clawbot_account_cursor_invalid",
                    "ClawBot 账号游标损坏，请重新发送一条消息",
                )
            })?;
        state_validate(&state)?;
        Ok(Self {
            channel: account,
            state,
            cursor,
        })
    }

    pub fn id(&self) -> &ChannelAccountId {
        &self.channel.id
    }

    pub fn bot_id_hint(&self) -> &str {
        &self.state.bot_id_hint
    }

    pub fn user_id_hint(&self) -> &str {
        &self.state.user_id_hint
    }

    pub fn base_url(&self) -> &str {
        &self.state.base_url
    }

    pub fn state(&self) -> &ClawBotAccountState {
        &self.state
    }

    pub fn cursor(&self) -> &ClawBotCursor {
        &self.cursor
    }

    pub fn channel_account(&self) -> &ChannelAccount {
        &self.channel
    }

    pub fn bot_token_secret_ref(&self) -> Result<SecretRef, ChannelError> {
        bot_token_secret_ref(&self.channel.id)
    }

    pub fn context_token_secret_ref(&self) -> Result<SecretRef, ChannelError> {
        context_token_secret_ref(&self.channel.id)
    }

    pub fn with_base_url(
        mut self,
        base_url: impl Into<String>,
        updated_at: Timestamp,
    ) -> Result<Self, ChannelError> {
        let base_url = normalize_platform_id("base_url", &base_url.into())?;
        self.state.base_url = base_url;
        self.channel.updated_at = updated_at;
        sync_serialized_state(&mut self.channel, &self.state, &self.cursor)?;
        self.channel.display_name = display_name(&self.state);
        Ok(self)
    }

    pub fn into_channel_account(mut self) -> Result<ChannelAccount, ChannelError> {
        sync_serialized_state(&mut self.channel, &self.state, &self.cursor)?;
        Ok(self.channel)
    }
}

pub fn stable_account_id(
    bot_id: impl AsRef<str>,
    user_id: impl AsRef<str>,
) -> Result<ChannelAccountId, ChannelError> {
    let bot_id = normalize_platform_id("bot_id", bot_id.as_ref())?;
    let user_id = normalize_platform_id("user_id", user_id.as_ref())?;
    let mut digest = Sha256::new();
    digest.update(CLAWBOT_CHANNEL_ID.as_bytes());
    digest.update([0]);
    digest.update(bot_id.as_bytes());
    digest.update([0]);
    digest.update(user_id.as_bytes());
    let digest = digest.finalize();
    let mut prefix = String::with_capacity(ACCOUNT_ID_HASH_HEX_LENGTH);
    for byte in digest {
        write!(prefix, "{byte:02x}").expect("写入 String 不会失败");
        if prefix.len() == ACCOUNT_ID_HASH_HEX_LENGTH {
            break;
        }
    }
    ChannelAccountId::new(format!("{CLAWBOT_CHANNEL_ID}-{prefix}")).map_err(|_| {
        ChannelError::permanent("clawbot_account_id_invalid", "ClawBot 账号标识生成失败")
    })
}

pub fn bot_token_secret_ref(account_id: &ChannelAccountId) -> Result<SecretRef, ChannelError> {
    secret_ref(account_id, "bot-token")
}

pub fn context_token_secret_ref(account_id: &ChannelAccountId) -> Result<SecretRef, ChannelError> {
    secret_ref(account_id, "context-token")
}

fn secret_ref(account_id: &ChannelAccountId, suffix: &str) -> Result<SecretRef, ChannelError> {
    SecretRef::new(format!("clawbot/{}/{suffix}", account_id.as_str()))
}

fn state_validate(state: &ClawBotAccountState) -> Result<(), ChannelError> {
    for (field, value) in [
        ("bot_id_hint", state.bot_id_hint.as_str()),
        ("user_id_hint", state.user_id_hint.as_str()),
        ("base_url", state.base_url.as_str()),
    ] {
        if value.trim().is_empty() {
            return Err(ChannelError::permanent(
                "clawbot_account_state_invalid",
                format!("ClawBot 账号状态字段 {field} 不能为空"),
            ));
        }
    }
    Ok(())
}

fn normalize_platform_id(field: &str, value: &str) -> Result<String, ChannelError> {
    let value = value.trim();
    if value.is_empty() {
        return Err(ChannelError::permanent(
            "clawbot_platform_id_invalid",
            format!("ClawBot 平台字段 {field} 不能为空"),
        ));
    }
    Ok(value.into())
}

fn tail_hint(value: &str) -> String {
    let mut characters = value
        .chars()
        .rev()
        .take(PLATFORM_ID_HINT_CHARACTERS)
        .collect::<Vec<_>>();
    characters.reverse();
    characters.into_iter().collect()
}

fn display_name(state: &ClawBotAccountState) -> String {
    format!("ClawBot 微信 · ...{}", state.user_id_hint)
}

fn sync_serialized_state(
    channel: &mut ChannelAccount,
    state: &ClawBotAccountState,
    cursor: &ClawBotCursor,
) -> Result<(), ChannelError> {
    channel.config = serde_json::to_value(state).map_err(|_| {
        ChannelError::permanent("clawbot_account_state_invalid", "ClawBot 账号状态编码失败")
    })?;
    channel.cursor = serde_json::to_value(cursor).map_err(|_| {
        ChannelError::permanent("clawbot_account_cursor_invalid", "ClawBot 账号游标编码失败")
    })?;
    Ok(())
}
