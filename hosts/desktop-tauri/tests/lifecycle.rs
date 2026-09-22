use std::{
    collections::VecDeque,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
};

use agentnotify_desktop::lifecycle::{
    autostart::{AutostartAction, CurrentUserAutostart, set_autostart},
    pause::{JsonPauseSettingsStore, PauseCoordinator, PauseSettingsStore, RuntimeControl},
    single_instance::{SecondInstanceAction, second_instance_action},
    tray::{TrayMenuAction, tray_menu},
    window::{
        LifecycleAction, RuntimeReadyAction, RuntimeState, WindowAction,
        determine_runtime_ready_action, replacement_for_quit, runtime_ready_action,
        window_action_for_close,
    },
};

#[test]
fn close_request_hides_window_while_runtime_is_running() {
    assert_eq!(
        window_action_for_close(RuntimeState::Running, true),
        WindowAction::Hide
    );
}

#[test]
fn close_request_is_allowed_only_without_a_tray() {
    assert_eq!(
        window_action_for_close(RuntimeState::Running, false),
        WindowAction::AllowClose
    );
}

#[test]
fn runtime_ready_shows_the_previously_hidden_main_window() {
    assert_eq!(runtime_ready_action(false), RuntimeReadyAction::KeepHidden);
    assert_eq!(runtime_ready_action(true), RuntimeReadyAction::ShowMain);
    assert_eq!(
        determine_runtime_ready_action(false, false),
        RuntimeReadyAction::ShowMain
    );
    assert_eq!(
        determine_runtime_ready_action(true, false),
        RuntimeReadyAction::KeepHidden
    );
    assert_eq!(
        determine_runtime_ready_action(false, true),
        RuntimeReadyAction::KeepHidden
    );
    assert_eq!(
        determine_runtime_ready_action(true, true),
        RuntimeReadyAction::KeepHidden
    );
}

#[test]
fn quit_request_stops_runtime_before_exit() {
    assert_eq!(
        replacement_for_quit(RuntimeState::Running),
        LifecycleAction::ShutdownRuntime
    );
}

#[test]
fn second_launch_only_shows_the_existing_main_window() {
    assert_eq!(
        second_instance_action(),
        SecondInstanceAction::ShowExistingMain
    );
}

#[test]
fn tray_menu_uses_fixed_actions_and_toggles_pause_label() {
    let running = tray_menu(false);
    assert_eq!(running.show.label(), "显示 AgentNotify");
    assert_eq!(running.pause.label(), "暂停通知");
    assert_eq!(running.quit.label(), "退出");

    let paused = tray_menu(true);
    assert_eq!(paused.pause.label(), "恢复通知");

    assert_eq!(running.pause.action(), TrayMenuAction::SetPaused(true));
    assert_eq!(paused.pause.action(), TrayMenuAction::SetPaused(false));
}

#[test]
fn autostart_adapter_only_exposes_current_user_actions() {
    let adapter = Arc::new(MemoryAutostart::default());

    set_autostart(adapter.as_ref(), true).expect("启用当前用户自启动必须成功");
    set_autostart(adapter.as_ref(), false).expect("禁用当前用户自启动必须成功");

    assert_eq!(
        adapter.actions(),
        vec![
            AutostartAction::EnableCurrentUser,
            AutostartAction::DisableCurrentUser
        ]
    );
}

#[tokio::test]
async fn startup_restores_pause_to_the_outbox_gate_only() {
    let runtime = Arc::new(MemoryRuntime::default());
    let settings = Arc::new(MemorySettings::with_paused(true));

    let coordinator = PauseCoordinator::initialize(runtime.clone(), settings)
        .await
        .expect("启动时必须恢复暂停边界");

    assert!(coordinator.is_paused());
    assert!(runtime.outbox_paused());
    assert_eq!(runtime.ingress_accepts(), 0);
    assert_eq!(runtime.ingress_spooled(), 0);
}

#[tokio::test]
async fn pause_persists_state_and_leaves_ingress_spool_running() {
    let runtime = Arc::new(MemoryRuntime::default());
    let settings = Arc::new(MemorySettings::default());
    let coordinator = PauseCoordinator::initialize(runtime.clone(), settings.clone())
        .await
        .expect("暂停适配器必须可初始化");

    runtime.spool_ingress("first");
    coordinator.set_paused(true).await.expect("暂停必须成功");
    runtime.spool_ingress("second");

    assert!(settings.paused());
    assert!(runtime.outbox_paused());
    assert_eq!(runtime.ingress_spooled(), 2);
    assert_eq!(runtime.claimed(), Vec::<String>::new());

    coordinator.set_paused(false).await.expect("恢复必须成功");
    runtime.spool_ingress("third");

    assert_eq!(
        runtime.claim_next(),
        Some("first".into()),
        "恢复后必须按原顺序继续"
    );
    assert_eq!(runtime.claim_next(), Some("second".into()));
    assert_eq!(runtime.claim_next(), Some("third".into()));
}

#[tokio::test]
async fn pause_persistence_failure_does_not_change_runtime_gate() {
    let runtime = Arc::new(MemoryRuntime::default());
    let settings = Arc::new(MemorySettings::failing_save());
    let coordinator = PauseCoordinator::initialize(runtime.clone(), settings)
        .await
        .expect("暂停适配器必须可初始化");

    let error = coordinator
        .set_paused(true)
        .await
        .expect_err("持久化失败必须明确返回错误");

    assert_eq!(error.code(), "pause_settings_save_failed");
    assert!(!coordinator.is_paused());
    assert!(!runtime.outbox_paused());
}

#[tokio::test]
async fn pause_settings_round_trip_only_uses_an_isolated_directory() {
    let root = tempfile::Builder::new()
        .prefix("agentnotify-lifecycle-settings-")
        .tempdir_in(agentnotify_testkit::test_temp_root())
        .expect("测试临时目录必须可创建");
    std::fs::write(root.path().join("settings.json"), r#"{"autoStart":true}"#)
        .expect("测试设置文件必须可写入");
    let settings = JsonPauseSettingsStore::new(root.path());

    settings
        .save_paused(true)
        .await
        .expect("暂停状态必须可保存");
    assert!(settings.load_paused().await.expect("暂停状态必须可读取"));

    let persisted: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(root.path().join("settings.json")).expect("设置文件必须存在"),
    )
    .expect("设置文件必须是有效 JSON");
    assert_eq!(persisted["notificationsPaused"], true);
    assert_eq!(persisted["autoStart"], true);
}

#[tokio::test]
async fn unavailable_outbox_control_is_not_reported_as_paused() {
    let runtime = Arc::new(MemoryRuntime::with_pause_support(false));
    let settings = Arc::new(MemorySettings::default());
    let coordinator = PauseCoordinator::initialize(runtime, settings)
        .await
        .expect("初始化不应伪造暂停支持");

    let error = coordinator
        .set_paused(true)
        .await
        .expect_err("缺少核心控制面时必须失败");

    assert_eq!(error.code(), "outbox_pause_unavailable");
    assert!(!coordinator.is_paused());
}

#[tokio::test]
async fn quit_awaits_runtime_shutdown_before_acknowledging_exit() {
    let runtime = Arc::new(MemoryRuntime::default());
    let settings = Arc::new(MemorySettings::default());
    let coordinator = PauseCoordinator::initialize(runtime.clone(), settings)
        .await
        .expect("暂停适配器必须可初始化");

    coordinator
        .shutdown_runtime()
        .await
        .expect("退出必须先关闭 runtime");

    assert_eq!(runtime.shutdowns(), 1);
}

#[derive(Default)]
struct MemoryAutostart {
    actions: Mutex<Vec<AutostartAction>>,
}

impl MemoryAutostart {
    fn actions(&self) -> Vec<AutostartAction> {
        self.actions.lock().expect("自启动测试锁不应中毒").clone()
    }
}

impl CurrentUserAutostart for MemoryAutostart {
    fn set_current_user_enabled(
        &self,
        enabled: bool,
    ) -> Result<(), agentnotify_desktop::lifecycle::LifecycleError> {
        self.actions
            .lock()
            .expect("自启动测试锁不应中毒")
            .push(if enabled {
                AutostartAction::EnableCurrentUser
            } else {
                AutostartAction::DisableCurrentUser
            });
        Ok(())
    }
}

#[derive(Default)]
struct MemorySettings {
    paused: AtomicBool,
    fail_save: bool,
}

impl MemorySettings {
    fn with_paused(paused: bool) -> Self {
        Self {
            paused: AtomicBool::new(paused),
            fail_save: false,
        }
    }

    fn failing_save() -> Self {
        Self {
            paused: AtomicBool::new(false),
            fail_save: true,
        }
    }

    fn paused(&self) -> bool {
        self.paused.load(Ordering::SeqCst)
    }
}

#[async_trait::async_trait]
impl PauseSettingsStore for MemorySettings {
    async fn load_paused(&self) -> Result<bool, agentnotify_desktop::lifecycle::LifecycleError> {
        Ok(self.paused())
    }

    async fn save_paused(
        &self,
        paused: bool,
    ) -> Result<(), agentnotify_desktop::lifecycle::LifecycleError> {
        if self.fail_save {
            return Err(agentnotify_desktop::lifecycle::LifecycleError::new(
                "pause_settings_save_failed",
                "暂停设置保存失败",
            ));
        }
        self.paused.store(paused, Ordering::SeqCst);
        Ok(())
    }
}

struct MemoryRuntime {
    outbox_paused: AtomicBool,
    pause_supported: AtomicBool,
    ingress_spool: Mutex<VecDeque<String>>,
    claimed: Mutex<Vec<String>>,
    ingress_spooled: AtomicUsize,
    shutdowns: AtomicUsize,
}

impl Default for MemoryRuntime {
    fn default() -> Self {
        Self {
            outbox_paused: AtomicBool::new(false),
            pause_supported: AtomicBool::new(true),
            ingress_spool: Mutex::new(VecDeque::new()),
            claimed: Mutex::new(Vec::new()),
            ingress_spooled: AtomicUsize::new(0),
            shutdowns: AtomicUsize::new(0),
        }
    }
}

impl MemoryRuntime {
    fn with_pause_support(supported: bool) -> Self {
        let runtime = Self::default();
        runtime.pause_supported.store(supported, Ordering::SeqCst);
        runtime
    }

    fn spool_ingress(&self, value: &str) {
        self.ingress_spool
            .lock()
            .expect("ingress 测试锁不应中毒")
            .push_back(value.into());
        self.ingress_spooled.fetch_add(1, Ordering::SeqCst);
    }

    fn claim_next(&self) -> Option<String> {
        if self.outbox_paused() {
            return None;
        }
        let value = self
            .ingress_spool
            .lock()
            .expect("ingress 测试锁不应中毒")
            .pop_front();
        if let Some(value) = &value {
            self.claimed
                .lock()
                .expect("Outbox 测试锁不应中毒")
                .push(value.clone());
        }
        value
    }

    fn outbox_paused(&self) -> bool {
        self.outbox_paused.load(Ordering::SeqCst)
    }

    fn claimed(&self) -> Vec<String> {
        self.claimed.lock().expect("Outbox 测试锁不应中毒").clone()
    }

    fn ingress_accepts(&self) -> usize {
        self.ingress_spooled.load(Ordering::SeqCst)
    }

    fn ingress_spooled(&self) -> usize {
        self.ingress_spooled.load(Ordering::SeqCst)
    }

    fn shutdowns(&self) -> usize {
        self.shutdowns.load(Ordering::SeqCst)
    }
}

#[async_trait::async_trait]
impl RuntimeControl for MemoryRuntime {
    fn state(&self) -> RuntimeState {
        RuntimeState::Running
    }
    async fn set_outbox_paused(
        &self,
        paused: bool,
    ) -> Result<(), agentnotify_desktop::lifecycle::LifecycleError> {
        if !self.pause_supported.load(Ordering::SeqCst) {
            return Err(agentnotify_desktop::lifecycle::LifecycleError::new(
                "outbox_pause_unavailable",
                "当前 runtime 尚未提供 Outbox 暂停控制",
            ));
        }
        self.outbox_paused.store(paused, Ordering::SeqCst);
        Ok(())
    }

    async fn shutdown_runtime(&self) -> Result<(), agentnotify_desktop::lifecycle::LifecycleError> {
        self.shutdowns.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
}

#[tokio::test]
async fn attach_runtime_with_action_preserves_action() {
    let controller = agentnotify_desktop::lifecycle::LifecycleController::new();
    let runtime = Arc::new(MemoryRuntime::default());
    let settings = Arc::new(MemorySettings::default());

    let action = controller
        .attach_runtime_with_action(runtime, settings, RuntimeReadyAction::KeepHidden)
        .await
        .expect("attach 必须成功");

    assert_eq!(action, RuntimeReadyAction::KeepHidden);
    assert!(controller.is_runtime_ready());
}
