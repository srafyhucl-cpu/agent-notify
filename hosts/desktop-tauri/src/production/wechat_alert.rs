//! 「微信推送已断」首次提醒（2026-09-17 设计，2.0 Rust 端补齐）。
//!
//! 平台会回收 ClawBot 的 `context_token`：此后主动推送全部失败（`ret=-2 prepare failed`），
//! 但登录仍然有效、界面以前看不出异常。这里随运行时启动一个低频巡检：
//! - 只针对 ClawBot 的「已登录但推送会话失效」状态（`clawbot_push_session_missing`）；
//!   从未就绪（刚登录等首条消息）不算，登录失效（ret=-14）也不算（由扫码提示承担）；
//! - 首次出现弹一次系统通知（把 `session_alert_at` 落库，**跨进程重启不重复弹**）；
//! - 会话恢复（重新拿到上下文）后由 session 轮询清除记录，再次失效可再弹。
//!
//! 巡检无条件地低频运行：不依赖用户打开渠道页，失败态出现后 1 分钟内即可提醒。

use std::sync::Arc;
use std::time::Duration;

use agentnotify_application::ChannelAccountStore;
use agentnotify_channel_clawbot::{CLAWBOT_CHANNEL_ID, ClawBotAccount, ClawBotAccountState};
use agentnotify_channel_sdk::{ChannelHealth, ChannelRegistry};
use agentnotify_domain::{ChannelAccountId, Timestamp};
use agentnotify_storage_sqlite::SqliteStore;
use tauri::{AppHandle, Wry};
use tauri_plugin_notification::NotificationExt;

/// 巡检间隔。
const ALERT_INTERVAL: Duration = Duration::from_secs(60);
/// 「推送已断」提示标题（用户可见文案，中文）。
const PUSH_BROKEN_TITLE: &str = "微信推送已断开";
/// 「推送已断」提示正文（与 2026-09-17 设计一致：写清现象与恢复办法）。
const PUSH_BROKEN_BODY: &str = "ClawBot 主动推送会话失效，任务通知暂时发不出去了。请在微信里给 ClawBot 发任意一条消息即可恢复。";
/// 需要提醒的健康明细码（渠道层判定「曾就绪但会话上下文缺失」）。
const PUSH_BROKEN_CODE: &str = "clawbot_push_session_missing";

/// 是否需要对「这次断开」弹提醒：健康为推送会话失效，且本轮尚未提醒过（纯函数，便于测试）。
pub fn push_broken_alert_needed(health: &ChannelHealth, state: &ClawBotAccountState) -> bool {
    health.available
        && health.stale
        && health
            .detail
            .as_ref()
            .is_some_and(|detail| detail.code() == PUSH_BROKEN_CODE)
        && state.session_alert_at.is_none()
}

/// 启动巡检任务（进程内单例；随进程退出结束）。
pub fn spawn(app: AppHandle<Wry>, store: Arc<SqliteStore>, registry: Arc<ChannelRegistry>) {
    static SPAWNED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    if SPAWNED
        .compare_exchange(
            false,
            true,
            std::sync::atomic::Ordering::SeqCst,
            std::sync::atomic::Ordering::SeqCst,
        )
        .is_err()
    {
        return;
    }
    tokio::spawn(async move {
        loop {
            let alerted = check_once(&app, &store, &registry).await;
            if alerted > 0 {
                tracing::info!(alerted, "已弹出「微信推送已断」提醒");
            }
            tokio::time::sleep(ALERT_INTERVAL).await;
        }
    });
}

/// 巡检一轮：返回本轮弹出的提醒数。
async fn check_once(
    app: &AppHandle<Wry>,
    store: &SqliteStore,
    registry: &ChannelRegistry,
) -> usize {
    let mut alerted = 0;
    for adapter in registry.all() {
        if adapter.descriptor().id.as_str() != CLAWBOT_CHANNEL_ID {
            continue;
        }
        let accounts = match store.list(&adapter.descriptor().id).await {
            Ok(accounts) => accounts,
            Err(error) => {
                tracing::warn!(code = error.code(), "推送已断巡检读取渠道账号失败：{error}");
                continue;
            }
        };
        for account in accounts {
            if !account.enabled {
                continue;
            }
            let health = adapter.inspect(account.clone()).await;
            let Ok(parsed) = ClawBotAccount::from_channel_account(account.clone()) else {
                continue;
            };
            if !push_broken_alert_needed(&health, parsed.state()) {
                continue;
            }
            // 弹一次系统通知：弹失败不落记录（下一轮巡检会重试）。
            if app
                .notification()
                .builder()
                .title(PUSH_BROKEN_TITLE)
                .body(PUSH_BROKEN_BODY)
                .show()
                .is_err()
            {
                tracing::warn!(
                    account = %account.id,
                    "弹出「微信推送已断」提醒失败，下次巡检重试"
                );
                continue;
            }
            match mark_alerted(store, &account.id).await {
                Ok(()) => alerted += 1,
                Err(error) => tracing::warn!(
                    account = %account.id,
                    "记录「微信推送已断」提醒时刻失败（重启后可能重复提醒）：{error}"
                ),
            }
        }
    }
    alerted
}

/// 把「已针对当前这次断开提醒过」写回账号状态（跨进程重启生效）。
async fn mark_alerted(store: &SqliteStore, account_id: &ChannelAccountId) -> Result<(), String> {
    let current = store
        .get(account_id)
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "账号不存在".to_string())?;
    let parsed = ClawBotAccount::from_channel_account(current.clone())
        .map_err(|error| error.message().to_string())?;
    let mut state = parsed.state().clone();
    state.session_alert_at = Some(Timestamp::now_utc());
    let mut updated = current;
    updated.config =
        serde_json::to_value(state).map_err(|error| format!("账号状态编码失败：{error}"))?;
    updated.updated_at = Timestamp::now_utc();
    store
        .upsert(updated)
        .await
        .map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use agentnotify_domain::SafeError;

    fn broken_health() -> ChannelHealth {
        ChannelHealth::stale(
            SafeError::new(
                PUSH_BROKEN_CODE,
                "ClawBot 主动推送会话已失效：请在微信里给 ClawBot 发任意一条消息即可恢复",
            )
            .expect("内置安全错误必须有效"),
        )
    }

    fn account_state(alerted: bool) -> ClawBotAccountState {
        let mut state = ClawBotAccountState::new("bot-1".into(), "user-1".into());
        state.session_established_at = Some(Timestamp::now_utc());
        state.session_alert_at = alerted.then(Timestamp::now_utc);
        state
    }

    /// 首次断开：需要提醒；已提醒过：不重复。
    #[test]
    fn alerts_once_per_break() {
        assert!(push_broken_alert_needed(
            &broken_health(),
            &account_state(false)
        ));
        assert!(!push_broken_alert_needed(
            &broken_health(),
            &account_state(true)
        ));
    }

    /// 健康 / 登录失效 / 从未就绪：都不弹提醒。
    #[test]
    fn no_alert_outside_push_session_missing() {
        assert!(!push_broken_alert_needed(
            &ChannelHealth::healthy(),
            &account_state(false)
        ));
        let invalid_login = ChannelHealth::stale(
            SafeError::new("clawbot_session_stale", "ClawBot 登录已失效，请重新扫码")
                .expect("内置安全错误必须有效"),
        );
        assert!(!push_broken_alert_needed(
            &invalid_login,
            &account_state(false)
        ));
        let unavailable = ChannelHealth::unavailable(
            SafeError::new(
                "clawbot_secret_not_found",
                "未找到该账号的渠道密钥，请重新登录",
            )
            .expect("内置安全错误必须有效"),
        );
        assert!(!push_broken_alert_needed(
            &unavailable,
            &account_state(false)
        ));
    }
}
