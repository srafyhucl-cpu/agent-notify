//! 生产退出端口：安装器拉起成功后与「退出应用」命令共用的优雅关闭路径。
//! 只负责“排期退出”这一件事，停运行时、checkpoint WAL、退进程的顺序固定。

use std::sync::Arc;
use std::time::Duration;

use agentnotify_storage_sqlite::SqliteStore;
use tauri::{AppHandle, Manager, Wry};

use super::runtime::ProductionRuntimeCoordinator;
use crate::lifecycle::LifecycleController;
use crate::update::AppExitRequester;

/// 安装器拉起成功到应用退出之间的等待：先让"正在安装"的响应回到界面，再走优雅退出。
/// 必须明显短于安装器等待应用释放文件的窗口，避免安装器卡在关闭应用这一步。
const UPDATE_EXIT_DELAY: Duration = Duration::from_millis(1000);

/// 生产退出端口：安装器成功拉起后，复用"退出应用"命令的优雅关闭路径退出进程。
/// `request_exit` 只负责排期，不阻塞命令响应；测试装配（无窗口）时什么都不做。
pub(super) struct ProductionAppExitRequester {
    app: Option<AppHandle<Wry>>,
    runtime: Arc<ProductionRuntimeCoordinator>,
    store: Arc<SqliteStore>,
}

impl ProductionAppExitRequester {
    pub(super) fn new(
        app: Option<AppHandle<Wry>>,
        runtime: Arc<ProductionRuntimeCoordinator>,
        store: Arc<SqliteStore>,
    ) -> Self {
        Self {
            app,
            runtime,
            store,
        }
    }
}

impl AppExitRequester for ProductionAppExitRequester {
    fn request_exit(&self) {
        let Some(app) = self.app.clone() else {
            // headless/测试装配：没有可退出的进程，保持"不退出"语义。
            tracing::warn!("更新安装器已启动，但当前装配没有窗口，不会自动退出");
            return;
        };
        // 立刻标记"正在退出"：安装器可能同时通过 Restart Manager 请求关窗，
        // 这时应该真的关窗，而不是缩回托盘让安装器一直等下去。
        if let Some(controller) = app.try_state::<LifecycleController>() {
            controller.begin_quit();
        }
        spawn_graceful_exit(
            app,
            self.runtime.clone(),
            self.store.clone(),
            UPDATE_EXIT_DELAY,
        );
    }
}

/// 既有的优雅退出路径：停运行时 → checkpoint WAL → 退出进程。
/// 先等待 `delay` 再开始关闭，这样触发退出的命令响应能先回到界面；
/// 单实例互斥随进程退出自动释放。
pub(super) fn spawn_graceful_exit(
    app: AppHandle<Wry>,
    runtime: Arc<ProductionRuntimeCoordinator>,
    store: Arc<SqliteStore>,
    delay: Duration,
) {
    tauri::async_runtime::spawn(async move {
        if !delay.is_zero() {
            tokio::time::sleep(delay).await;
        }
        if let Some(controller) = app.try_state::<LifecycleController>() {
            let _ = controller.shutdown_for_quit().await;
        } else {
            let _ = runtime.shutdown_runtime().await;
        }
        let _ = store.wal_checkpoint_truncate().await;
        app.exit(0);
    });
}
