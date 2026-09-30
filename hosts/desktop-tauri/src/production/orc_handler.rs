//! 编排命令处理器：任务创建/开始/推进/汇报回注/派活/呈现（2026-09 从 `service.rs` 拆分）。
//!
//! 与 `service.rs`（桥接命令门面）解耦：本模块只做编排业务；桌面命令经
//! `HostCommandService` 实现转发到 [`OrcCommandHandler`]。
//!
//! 「集群功能 v1」职责（定稿设计 §2–§5）：
//! - 任务创建锁定模板（`template_id`）与工作目录（`working_dir`，必填且必须是已存在目录）；
//! - 任务相关命令按 `task.workflow_id` → 内置模板解析工作流（旧预设 id 兼容），
//!   再按 settings `orchestration.node_config` 合并节点 Agent/模型；
//! - 派活透传工作目录/该步模型/无人值守标志（`DispatchOptions`）；
//! - 最后一步完成不直接 Completed：进入「项目经理汇总阶段」，派汇总信封回首节点，
//!   首节点汇总产出到达后完成任务并推最终汇报。

use std::collections::{BTreeMap, HashMap};
use std::path::Path;
use std::sync::{Arc, Mutex as StdMutex};

use tokio::sync::{Mutex as AsyncMutex, OwnedMutexGuard};

use agentnotify_domain::{AgentId, AgentSessionId, Timestamp};
use agentnotify_orchestration::{
    MessageKind, NotifyMode, OrcError, OrcRepositoryError, OrcStore, OrcTask, OrcTaskRepository,
    StepOutcome, TaskState, TemplateResolver, TransitionAction, Workflow, render_step_reports,
    round_continue_input,
};
use agentnotify_storage_sqlite::SqliteStore;

use super::agent_driver::{AgentDriver, DispatchOptions};
use super::orc_node_config::{
    MODEL_AGENT_OPENCODE, NodeConfig, ORC_MODEL_AGENT_UNSUPPORTED, ORC_MODEL_INVALID,
    ORC_STEP_AGENT_MISSING, apply_steps_snapshot, is_provider_model, missing_agent_step,
    steps_snapshot_of, task_steps_snapshot, template_dtos, validate_template_steps,
};
use super::orc_notify::{
    OrcClusterPresenter, failure_body, finalizing_body, progress_body, render_cluster_message,
    should_notify, truncate_final_report,
};
use super::orc_wechat_route::state_cn;
use super::settings::{
    KEY_ORCHESTRATION_NODE_CONFIG, KEY_ORCHESTRATION_UNATTENDED, ProductionSettingsStore,
};
use crate::bridge::dto::*;
use crate::bridge::error::CommandError;

/// 模型目录数据源（可注入，B2）：模型下拉与节点思考强度白名单共用同一份真实数据。
///
/// 生产注入 [`DesktopModelCatalog`]（读本机 OpenCode）；测试注入假目录，既不依赖
/// 真实 OpenCode 进程，也不放宽生产校验（生产仍走同一实现）。
#[async_trait::async_trait]
pub trait OrcModelCatalog: Send + Sync {
    /// 读取可用模型；OpenCode 未运行等失败必须返回面向用户的明确错误（不静默放行）。
    async fn list_models(&self) -> Result<Vec<OpencodeModelDto>, CommandError>;
}

/// 默认模型目录：读取本机 OpenCode 桌面端服务（生产装配使用）。
pub struct DesktopModelCatalog;

#[async_trait::async_trait]
impl OrcModelCatalog for DesktopModelCatalog {
    async fn list_models(&self) -> Result<Vec<OpencodeModelDto>, CommandError> {
        super::opencode_models::list_models().await
    }
}

/// 编排命令处理器（P1-1，B 方案接线）。
///
/// 两种装配模式：
/// - **静态（测试/向后兼容）**：`store: Option<OrcStore>` 启动时一次性定（`new`/`with_templates`
///   /`with_presenter`/`with_driver`），`orchestration.enabled` 由启动装配决定，改设置需重启；
///   任务的工作流 id 与装配工作流一致时沿用装配工作流（保留自定义工作流与测试变体）。
/// - **动态（生产，P1-1 体验修正）**：`with_selector` 注入 `Arc<SqliteStore>` + settings——
///   每次命令时按 `orchestration.enabled`（**默认开启**）实时解析，**改设置立即生效，无需重启**；
///   任务相关命令再按 `task.workflow_id` 解析内置模板并合并节点配置。
pub struct OrcCommandHandler {
    store: Option<OrcStore>,
    /// 动态模式：命令时按 settings 解析 enabled 构建/复用仓储（`repository`）。
    sqlite: Option<Arc<SqliteStore>>,
    settings: Option<ProductionSettingsStore>,
    /// harness 模板解析器（P1-5）：用户模板优先、内置默认兜底；派活时由此生成任务信封。
    templates: TemplateResolver,
    /// 集群消息呈现（P1-4）：缺省不呈现（行为与 P1-3 一致）；注入后 advance/mark_blocked 按通知节奏外发。
    presenter: Option<Arc<dyn OrcClusterPresenter>>,
    /// 派活驱动器（P2）：缺省不派活（行为与 P1-3/1-4 一致）；注入后 create/advance
    /// 把当前 Step 的任务信封真正交给配置的 Agent。
    driver: Option<Arc<dyn AgentDriver>>,
    /// 模型目录（B2）：variant 白名单校验与模型下拉共用；未注入回退本机 OpenCode。
    model_catalog: Option<Arc<dyn OrcModelCatalog>>,
    /// 任务级写锁（B1）：同一任务的「读改写」串行，避免并发写整任务覆盖丢更新。
    /// map 用 std 锁只做短查找（不跨 await）；值是 tokio 锁（允许跨 await 持有）。
    task_locks: StdMutex<HashMap<String, Arc<AsyncMutex<()>>>>,
}

/// 编排开关设置键（settings 表）。**默认开启**（缺失 = true，P1-1 体验修正：
/// 编排是桌面端核心能力，不应默认关掉让用户困惑）；显式 false 才关闭。
pub const KEY_ORCHESTRATION_ENABLED: &str = "orchestration.enabled";
/// 编排工作流选择设置键（settings 表）：`opencode-only` 只用 OpenCode 单 Agent；其它/缺失 = 默认多 Agent 委托。
/// 新任务不再使用该键（必须显式选模板）；仅旧 `get_current_orc_workflow` 预览沿用。
pub const KEY_ORCHESTRATION_WORKFLOW: &str = "orchestration.workflow";
/// 全局默认通知节奏设置键（settings 表，P1-4 §4.6）：缺失/非法回退 `final_only` 并告警。
pub const KEY_ORCHESTRATION_NOTIFY_MODE: &str = "orchestration.notify_mode";
/// 用户 harness 模板配置文件（`config_dir` 下，§4.3 / P1-5；缺失 = 内置默认兜底）。
pub const HARNESS_TEMPLATES_FILE: &str = "harness-templates.json";
const ORCHESTRATION_DISABLED_CODE: &str = "orchestration_disabled";
const ORCHESTRATION_DISABLED_MESSAGE: &str =
    "编排未启用：请在设置中启用 orchestration.enabled 后重启应用";
/// 创建任务的模板不存在（新任务只开放内置三档模板）。
const ORC_TEMPLATE_UNKNOWN: &str = "orc_template_unknown";
/// 任务的工作流无法解析（既不是装配工作流也不是内置模板）。
const ORC_WORKFLOW_UNKNOWN: &str = "orc_workflow_unknown";
/// 工作目录为空或不是已存在目录。
const ORC_WORKING_DIR_INVALID: &str = "orc_working_dir_invalid";
/// 任务名称非法（空 / 超过 8 字 / 含分隔符）。
const ORC_TASK_NAME_INVALID: &str = "orc_task_name_invalid";
/// 任务名称长度上限（字）：短名只用于列表与会话标题展示。
const ORC_TASK_NAME_MAX_CHARS: usize = 8;
/// 任务名禁用字符：`】`/`【` 会破坏微信指令 `【集群 <名>】` 的寻址（B3），创建/编辑一律拒绝。
const ORC_TASK_NAME_FORBIDDEN_CHARS: [char; 2] = ['【', '】'];
/// 思考强度不在该模型可选列表内（或模型未知）：`update_task_step` 白名单校验错误码（B2）。
const ORC_STEP_VARIANT_INVALID: &str = "orc_step_variant_invalid";
/// 模型标识长度上限（`provider/model` 全串）：防御异常超长输入塞爆步骤快照与日志（B2）。
const ORC_MODEL_MAX_CHARS: usize = 200;
/// 思考强度标识长度上限（OpenCode variant 短标识，如 high/xhigh）（B2）。
const ORC_VARIANT_MAX_CHARS: usize = 64;
/// 自动迭代轮次上限：项目经理判定「继续迭代」时最多自动跑到该轮次，之后按完成处理交给用户决定。
const ORC_MAX_ROUNDS: u32 = 5;
/// 任务处于「项目经理汇总阶段」：不接受人工推进（等待首节点汇总）。
const ORC_TASK_FINALIZING: &str = "orc_task_finalizing";
/// 任务步骤不可修改（终态只读 / 无步骤快照的旧任务，§12.4）。
pub const ORC_TASK_STEP_LOCKED: &str = "orc_task_step_locked";
/// 编排设置存储不可用（静态装配/初始化未完成）时保存节点配置的错误码。
const ORC_SETTINGS_UNAVAILABLE: &str = "orchestration_settings_unavailable";
/// `OrcError::task_not_found` 的稳定错误码（回注路径按它静默忽略缺失任务）。
const ORC_TASK_NOT_FOUND_CODE: &str = "orc.task_not_found";
/// 派活信封会话 id 前缀：`task-<task_id>-step-<n>`（每个 (task, step) 一个稳定会话）。
const ORC_DISPATCH_SESSION_PREFIX: &str = "task";
/// 插件把失败终态也上报为 `session.completed`，失败正文以该前缀开头（失败回合识别）。
const AGENT_FAILURE_BODY_PREFIX: &str = "任务执行失败：";
/// 汇总阶段无法派活的阻塞原因前缀（§4 失败语义）。
const SUMMARY_DISPATCH_FAILED_PREFIX: &str = "最终汇总：";
/// 首节点汇总回合失败的阻塞原因前缀（§4 失败语义）。
const SUMMARY_ROUND_FAILED_PREFIX: &str = "项目经理汇总失败：";

impl OrcCommandHandler {
    /// 默认装配：仅内置默认信封模板（向后兼容）。
    pub fn new(store: Option<OrcStore>) -> Self {
        Self::with_templates(store, TemplateResolver::new())
    }

    /// 装配用户 harness 模板解析器（用户模板优先、内置默认兜底，§4.3 / P1-5）。
    pub fn with_templates(store: Option<OrcStore>, templates: TemplateResolver) -> Self {
        Self {
            store,
            sqlite: None,
            settings: None,
            templates,
            presenter: None,
            driver: None,
            model_catalog: None,
            task_locks: StdMutex::new(HashMap::new()),
        }
    }

    /// 装配集群消息呈现（P1-4）：注入生产实现后，任务创建继承全局默认通知节奏，
    /// advance / mark_blocked 按 §4.6 决定是否外发微信。不注入 = 与 P1-3 行为一致。
    pub fn with_presenter(
        store: Option<OrcStore>,
        templates: TemplateResolver,
        presenter: Arc<dyn OrcClusterPresenter>,
    ) -> Self {
        Self {
            store,
            sqlite: None,
            settings: None,
            templates,
            presenter: Some(presenter),
            driver: None,
            model_catalog: None,
            task_locks: StdMutex::new(HashMap::new()),
        }
    }

    /// 完整装配（P2 派活链路）：呈现层 + 派活驱动器（静态模式，测试/向后兼容）。
    ///
    /// - `presenter=None` → 不推微信（与 P1-3 一致）；
    /// - `driver=None` → 只推进 + 呈现，不派活（与 P1-4 一致，向后兼容）。
    pub fn with_driver(
        store: Option<OrcStore>,
        templates: TemplateResolver,
        presenter: Option<Arc<dyn OrcClusterPresenter>>,
        driver: Option<Arc<dyn AgentDriver>>,
    ) -> Self {
        Self {
            store,
            sqlite: None,
            settings: None,
            templates,
            presenter,
            driver,
            model_catalog: None,
            task_locks: StdMutex::new(HashMap::new()),
        }
    }

    /// 动态装配（生产，P1-1 体验修正）：每次命令时按 settings 实时解析
    /// `orchestration.enabled`（**默认开启**），改设置立即生效无需重启。
    ///
    /// `store` 传入的 Option 仅用于静态测试路径（动态模式忽略）；生产传 `None`。
    pub fn with_selector(
        store: Option<OrcStore>,
        sqlite: Arc<SqliteStore>,
        settings: ProductionSettingsStore,
        templates: TemplateResolver,
        presenter: Option<Arc<dyn OrcClusterPresenter>>,
        driver: Option<Arc<dyn AgentDriver>>,
    ) -> Self {
        Self {
            store,
            sqlite: Some(sqlite),
            settings: Some(settings),
            templates,
            presenter,
            driver,
            model_catalog: None,
            task_locks: StdMutex::new(HashMap::new()),
        }
    }

    /// 注入模型目录（测试/生产装配）：variant 白名单校验用（B2）。
    pub fn with_model_catalog(mut self, catalog: Arc<dyn OrcModelCatalog>) -> Self {
        self.model_catalog = Some(catalog);
        self
    }

    /// 取任务级写锁（B1）：先短锁查/建该任务的锁，再返回 owned guard（可跨 await 持有）。
    ///
    /// 所有对外可变的任务命令必须先取此锁；内部 helper（`auto_blocked` / `dispatch_*` /
    /// `present_*`）及其调用的无锁内核不得重复取锁（会自锁死）。
    async fn task_lock(&self, task_id: &str) -> OwnedMutexGuard<()> {
        let lock = {
            let mut locks = self
                .task_locks
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            locks.entry(task_id.to_owned()).or_default().clone()
        };
        lock.lock_owned().await
    }

    /// 任务级写锁内核：已持锁时读改写任务（B1，`mark_settled_turn` 并入回注临界区）。
    async fn mark_settled_turn_locked(&self, task_id: &str, completed_at_ms: i64) {
        let (store, mut task) = match self.task_store(task_id).await {
            Ok(resolved) => resolved,
            Err(error) => {
                tracing::warn!(
                    task_id,
                    code = error.code(),
                    "回注后记录结算时刻失败：{}",
                    error.message()
                );
                return;
            }
        };
        if let Err(error) = task.mark_settled_turn(completed_at_ms) {
            tracing::warn!(
                task_id,
                code = error.code.as_str(),
                "记录回合结算时刻失败：{}",
                error.message
            );
            return;
        }
        if let Err(error) = store.save(task).await {
            tracing::warn!(task_id, "保存回合结算时刻失败：{error}");
        }
    }

    /// 模板解析器访问：派活生成任务信封时用（用户模板优先、内置默认兜底）。
    pub fn templates(&self) -> &TemplateResolver {
        &self.templates
    }

    /// 解析任务仓储（任务级解析用，克隆，代价可忽略）：
    ///
    /// - 静态模式（测试/向后兼容）：启动时装配的仓储原样复用（改设置需重启，语义不变）；
    /// - 动态模式（生产）：每命令按 `orchestration.enabled`（缺失/异常按 true，显式 false 才关闭）
    ///   实时判断；未启用返回明确错误 `orchestration_disabled`。
    async fn repository(&self) -> Result<Arc<dyn OrcTaskRepository>, CommandError> {
        if let Some(store) = &self.store {
            return Ok(store.repository());
        }
        let disabled =
            || CommandError::new(ORCHESTRATION_DISABLED_CODE, ORCHESTRATION_DISABLED_MESSAGE);
        let (Some(sqlite), Some(settings)) = (&self.sqlite, &self.settings) else {
            return Err(disabled());
        };
        match settings.store().settings_entries().await {
            Ok(entries) => {
                let enabled = entries
                    .get(KEY_ORCHESTRATION_ENABLED)
                    .and_then(serde_json::Value::as_bool)
                    .unwrap_or(true); // 默认开启（体验修正）。
                if !enabled {
                    return Err(disabled());
                }
            }
            Err(error) => {
                tracing::warn!(%error, "读取编排设置失败，按默认开启处理");
            }
        }
        Ok(sqlite.clone())
    }

    /// 任务级解析（§2/§3）：取任务 → 按 `workflow_id` 解析工作流 →（优先）任务快照 /
    /// （无快照）实时合并节点配置 → 绑定仓储。
    ///
    /// 快照锁定（§3「改配置不影响已创建任务」）：`start` 已快照的任务此后一律用快照，
    /// 运行期改 `orchestration.node_config` 不再影响它；未开始/旧任务无快照 = 实时合并（兼容）。
    async fn task_store(&self, task_id: &str) -> Result<(OrcStore, OrcTask), CommandError> {
        let repository = self.repository().await?;
        let task = OrcStore::fetch_task(&repository, task_id)
            .await
            .map_err(orc_error)?;
        let workflow = self.resolve_task_workflow(&task.workflow_id().map_err(orc_error)?)?;
        let effective = self.effective_workflow(&task, &workflow).await?;
        Ok((OrcStore::with_repository(effective, repository), task))
    }

    /// 任务生效工作流：有步骤快照（start 已锁定）→ 快照覆盖；否则实时合并节点配置。
    async fn effective_workflow(
        &self,
        task: &OrcTask,
        workflow: &Workflow,
    ) -> Result<Workflow, CommandError> {
        if let Some(snapshot) = task.steps_snapshot().map_err(orc_error)? {
            if !snapshot.is_empty() {
                return Ok(apply_steps_snapshot(workflow, &snapshot));
            }
        }
        Ok(self.node_config().await.merge_workflow(workflow))
    }

    /// 按任务记录的工作流 id 解析工作流（旧预设 id 兼容）：
    /// 静态装配的工作流 id 一致时沿用装配工作流（保留自定义/测试变体），否则回退内置模板；
    /// 未知 → `orc_workflow_unknown`（不猜测兜底）。
    fn resolve_task_workflow(&self, workflow_id: &str) -> Result<Workflow, CommandError> {
        if let Some(store) = &self.store {
            if store.workflow().id == workflow_id {
                return Ok(store.workflow().clone());
            }
        }
        Workflow::builtin(workflow_id).ok_or_else(|| {
            CommandError::new(
                ORC_WORKFLOW_UNKNOWN,
                format!("任务的工作流无法解析：{workflow_id}（可能来自不兼容的版本）"),
            )
        })
    }

    /// 创建任务时解析模板：静态装配的工作流 id 一致时沿用装配工作流（测试/自定义），
    /// 否则必须是内置三档模板；未知 → `orc_template_unknown`。
    fn resolve_create_workflow(&self, template_id: &str) -> Result<Workflow, CommandError> {
        if let Some(store) = &self.store {
            if store.workflow().id == template_id {
                return Ok(store.workflow().clone());
            }
        }
        Workflow::builtin(template_id).ok_or_else(|| {
            CommandError::new(
                ORC_TEMPLATE_UNKNOWN,
                format!("模板不存在：{template_id}（可选：快速修复 / 标准交付 / 完整评估）"),
            )
        })
    }

    /// 读取节点配置（§3）：settings 缺失 = 未配置；读取/解析失败 → 告警 + 按未配置处理（不猜）。
    pub async fn node_config(&self) -> NodeConfig {
        let Some(settings) = &self.settings else {
            return NodeConfig::default();
        };
        let entries = match settings.store().settings_entries().await {
            Ok(entries) => entries,
            Err(error) => {
                tracing::warn!(%error, "读取编排节点配置失败，按未配置处理");
                return NodeConfig::default();
            }
        };
        let Some(value) = entries.get(KEY_ORCHESTRATION_NODE_CONFIG) else {
            return NodeConfig::default();
        };
        match NodeConfig::from_json(value) {
            Ok(config) => config,
            Err(reason) => {
                tracing::warn!("编排节点配置读取失败，按未配置处理：{reason}");
                NodeConfig::default()
            }
        }
    }

    /// 无人值守开关（§6 `orchestration.unattended`）：缺失/读取失败按默认 true（不静默关闭）。
    pub async fn unattended(&self) -> bool {
        let Some(settings) = &self.settings else {
            return true;
        };
        match settings.store().settings_entries().await {
            Ok(entries) => entries
                .get(KEY_ORCHESTRATION_UNATTENDED)
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(true),
            Err(error) => {
                tracing::warn!(%error, "读取编排无人值守设置失败，按默认开启处理");
                true
            }
        }
    }

    pub async fn create(&self, payload: CreateOrcTaskPayload) -> Result<OrcTaskDto, CommandError> {
        let repository = self.repository().await?;
        let goal = payload.goal.trim();
        if goal.is_empty() {
            return Err(CommandError::new("orc_goal_empty", "任务目标不能为空"));
        }
        // 任务名称（必填、≤8 字）：新 UI 显式提交；旧调用方缺省时按目标前 8 字推导（向后兼容）。
        let name = match payload.name.as_deref() {
            Some(raw) => validate_task_name(raw)?,
            None => derive_task_name(goal),
        };
        // 任务锁定模板（§3.1）：只接受内置三档模板（或静态装配的工作流）。
        let workflow = self.resolve_create_workflow(payload.template_id.trim())?;
        let working_dir = payload.working_dir.trim();
        if working_dir.is_empty() {
            return Err(CommandError::new(
                ORC_WORKING_DIR_INVALID,
                "工作目录不能为空：请选择 OpenCode 项目或手动输入目录",
            ));
        }
        if !Path::new(working_dir).is_dir() {
            return Err(CommandError::new(
                ORC_WORKING_DIR_INVALID,
                format!("工作目录不存在：{working_dir}"),
            ));
        }
        // P1-4：显式指定 → 任务级覆盖（校验失败明确报错）；未指定 → 继承全局默认
        // `orchestration.notify_mode`（缺失/非法回退 final_only 并告警，§4.6）。
        let notify_mode = match payload.notify_mode.as_deref() {
            Some(raw) => parse_notify_mode(Some(raw))?,
            None => self.global_default_notify_mode().await,
        };
        // 节点配置来源（§3）：
        // - 新流程（创建任务弹窗逐个节点确认，payload.steps 有值）：任务级提交，**创建即快照锁定**，
        //   此后改设置不影响该任务（所见即所得）；
        // - 旧调用方（无 steps）：保持原行为——按当前设置实时合并，start 时才快照。
        let (merged, locked_snapshot) = match payload.steps.as_deref() {
            Some(steps) => {
                let snapshot = task_steps_snapshot(&workflow, steps)?;
                (apply_steps_snapshot(&workflow, &snapshot), Some(snapshot))
            }
            None => (self.node_config().await.merge_workflow(&workflow), None),
        };
        let store = OrcStore::with_repository(merged, repository);
        let mut task = store
            .create_task(goal, notify_mode)
            .await
            .map_err(orc_error)?;
        task.set_name(&name).map_err(orc_error)?;
        task.set_working_dir(working_dir).map_err(orc_error)?;
        // 创建时间（轮次时间线/列表展示用；旧任务缺省不显示）。
        task.set_created_at(&Timestamp::now_utc().to_rfc3339())
            .map_err(orc_error)?;
        if let Some(snapshot) = locked_snapshot {
            task.set_steps_snapshot(&snapshot).map_err(orc_error)?;
        }
        store.save(task.clone()).await.map_err(orc_error)?;
        // 创建后**不自动派活**：任务先进入「待开始」（started=false），用户在界面上
        // 看清工作流节点（每步角色与派给谁）后点「开始执行」再派活第 1 步。
        // 这样避免"没看清节点就被派活"，也避免默认工作流与实际可用 Agent 不匹配时的意外阻塞。
        orc_task_to_dto(&task, store.workflow())
    }

    /// 更新任务（集群页「编辑」）：名称随时可改；描述仅未开始任务可改；通知节奏随时可改。
    /// 未提供（None）的字段保持不变；全部校验通过才落库（失败不产生部分更新）。
    pub async fn update(&self, payload: UpdateOrcTaskPayload) -> Result<OrcTaskDto, CommandError> {
        let _guard = self.task_lock(&payload.task_id).await;
        let (store, mut task) = self.task_store(&payload.task_id).await?;
        if let Some(raw) = payload.name.as_deref() {
            let name = validate_task_name(raw)?;
            task.set_name(&name).map_err(orc_error)?;
        }
        if let Some(raw) = payload.goal.as_deref() {
            let goal = raw.trim();
            if goal.is_empty() {
                return Err(CommandError::new("orc_goal_empty", "任务描述不能为空"));
            }
            if task.is_started().map_err(orc_error)? {
                return Err(CommandError::new(
                    "orc_task_already_started",
                    "任务已开始，描述不可修改（可修改名称）",
                ));
            }
            task.set_goal(goal).map_err(orc_error)?;
        }
        if let Some(raw) = payload.notify_mode.as_deref() {
            let mode = parse_notify_mode(Some(raw))?;
            task.set_notify_mode(mode).map_err(orc_error)?;
        }
        store.save(task.clone()).await.map_err(orc_error)?;
        orc_task_to_dto(&task, store.workflow())
    }

    /// 修改任务某一步的模型/思考强度（§12.4，任务结束前可改）：
    /// 只改**任务步骤快照**，不改 Agent 与工作流结构——
    /// - 终态（Completed/Canceled/Rejected）只读；阻塞（Failed）/等待确认（InputRequired）可改后重新发起；
    /// - 旧任务没有步骤快照（或快照缺该步）→ 明确报错，指向「设置 → 编排」或重新创建；
    /// - `model=None` 不改；空串 = 清除（回到该 Agent 默认模型）；非空必须是 `provider/model`
    ///   且该步 Agent 支持指定模型（v1 仅 OpenCode）；
    /// - 强度随模型走：模型为空时强度一并清空；`variant=None` 不改、空串 = 清除，
    ///   非空 trim 后必须在模型 `variants` 白名单内（B2，目录不可用/模型未知/不在列表明确报错）。
    pub async fn update_task_step(
        &self,
        payload: UpdateOrcTaskStepPayload,
    ) -> Result<OrcTaskDto, CommandError> {
        let _guard = self.task_lock(&payload.task_id).await;
        let (store, mut task) = self.task_store(&payload.task_id).await?;
        if matches!(
            task.state(),
            TaskState::Completed | TaskState::Canceled | TaskState::Rejected
        ) {
            return Err(CommandError::new(
                ORC_TASK_STEP_LOCKED,
                "任务已结束：不能再修改节点模型",
            ));
        }
        let Some(step) = store.workflow().step(payload.order) else {
            return Err(orc_error(OrcError::step_not_found(payload.order)));
        };
        let agent = step.agent_hint.clone();
        let Some(mut snapshot) = task.steps_snapshot().map_err(orc_error)? else {
            return Err(CommandError::new(
                ORC_TASK_STEP_LOCKED,
                "该任务没有步骤快照（旧任务）：请到设置 → 编排里改模板，或重新创建任务",
            ));
        };
        let Some(entry) = snapshot
            .iter_mut()
            .find(|entry| entry.order == payload.order)
        else {
            return Err(CommandError::new(
                ORC_TASK_STEP_LOCKED,
                format!(
                    "该任务缺少第 {} 步的快照（旧任务）：请到设置 → 编排里改模板，或重新创建任务",
                    payload.order
                ),
            ));
        };
        // model：None = 不改；空串 = 清除；非空 = 校验格式与 Agent 支持后写入。
        let model = match payload.model.as_deref() {
            None => entry.model.clone(),
            Some(raw) => {
                let trimmed = raw.trim();
                if trimmed.is_empty() {
                    None
                } else {
                    if !is_provider_model(trimmed) {
                        return Err(CommandError::new(
                            ORC_MODEL_INVALID,
                            "模型格式应为 provider/model（例如 opencode-go/deepseek-v4.1-flash）",
                        ));
                    }
                    if trimmed.chars().count() > ORC_MODEL_MAX_CHARS {
                        return Err(CommandError::new(
                            ORC_MODEL_INVALID,
                            format!(
                                "模型标识过长（最多 {ORC_MODEL_MAX_CHARS} 个字符）：请从下拉列表重新选择"
                            ),
                        ));
                    }
                    if agent.as_deref() != Some(MODEL_AGENT_OPENCODE) {
                        return Err(CommandError::new(
                            ORC_MODEL_AGENT_UNSUPPORTED,
                            "该 Agent 暂不支持指定模型",
                        ));
                    }
                    Some(trimmed.to_string())
                }
            }
        };
        // variant：模型为空时一并清空（强度依附于模型）；否则 None = 不改（保持旧值）、
        // 空串 = 清除、非空 trim 后必须在该模型 `variants` 白名单内（B2：目录不可用/模型未知/
        // 不在列表都明确报错，不静默放行；未提交强度时不校验，兼容旧任务）。
        let variant = match (model.as_deref(), payload.variant.as_deref()) {
            (None, _) => None,
            (Some(_), None) => entry.variant.clone(),
            (Some(model_id), Some(raw)) => {
                let trimmed = raw.trim();
                if trimmed.is_empty() {
                    None
                } else {
                    if trimmed.chars().count() > ORC_VARIANT_MAX_CHARS {
                        return Err(CommandError::new(
                            ORC_STEP_VARIANT_INVALID,
                            format!(
                                "思考强度标识过长（最多 {ORC_VARIANT_MAX_CHARS} 个字符）：请从下拉列表重新选择"
                            ),
                        ));
                    }
                    let models = self.list_models().await?;
                    validate_variant(&models, model_id, trimmed)?;
                    Some(trimmed.to_string())
                }
            }
        };
        entry.model = model;
        entry.variant = variant;
        task.set_steps_snapshot(&snapshot).map_err(orc_error)?;
        store.save(task.clone()).await.map_err(orc_error)?;
        // 返回的 DTO 必须体现本次改动：把新快照重新应用到生效工作流上再脱敏。
        let effective = apply_steps_snapshot(store.workflow(), &snapshot);
        orc_task_to_dto(&task, &effective)
    }

    /// 删除任务（集群页「删除」，幂等）：删除任务记录，不做猜测式兜底。
    /// 已派活的 OpenCode 会话不会被停止（界面确认文案里明确说明）。
    pub async fn delete(
        &self,
        payload: OrcTaskIdPayload,
    ) -> Result<MutationAcceptedDto, CommandError> {
        let _guard = self.task_lock(&payload.task_id).await;
        let repository = self.repository().await?;
        repository
            .delete_task(payload.task_id.trim())
            .await
            .map_err(|error| CommandError::new(error.code(), error.message().to_string()))?;
        Ok(MutationAcceptedDto {
            accepted: true,
            id: None,
        })
    }

    /// 开始执行（人工确认后）：预检全部节点 Agent 已配置、工作目录仍存在，
    /// **把合并后的步骤配置快照进任务**（此后改设置不影响该任务），然后标记已开始并派活第 1 步（§3.1）。
    /// 新流程任务在创建时已锁定快照（payload.steps），这里幂等重写；旧任务在此首次锁定。
    ///
    /// 预检失败 → 明确报错且任务保持「待开始」（不写快照）；已开始的任务再调 → 明确错误。
    pub async fn start(&self, payload: OrcTaskIdPayload) -> Result<OrcTaskDto, CommandError> {
        let _guard = self.task_lock(&payload.task_id).await;
        let (store, mut task) = self.task_store(&payload.task_id).await?;
        if task.is_started().map_err(orc_error)? {
            return Err(CommandError::new(
                "orc_task_already_started",
                "任务已开始执行，无需重复开始",
            ));
        }
        // 预检 1：合并后所有节点 Agent 必须已配置（避免跑到一半才失败，§3）。
        if let Some(order) = missing_agent_step(store.workflow()) {
            return Err(CommandError::new(
                ORC_STEP_AGENT_MISSING,
                format!("第 {order} 步未选择 Agent：请先在设置 → 编排中配置"),
            ));
        }
        // 预检 2：工作目录仍存在（旧任务无工作目录 = 跟随宿主当前项目，不校验）。
        if let Some(dir) = task.working_dir().map_err(orc_error)? {
            if !Path::new(&dir).is_dir() {
                return Err(CommandError::new(
                    ORC_WORKING_DIR_INVALID,
                    format!("工作目录不存在：{dir}"),
                ));
            }
        }
        // 锁定运行期配置（§3）：快照 = 此刻实时合并后的步骤 Agent/模型；
        // 此后 dispatch/校验/DTO 展示优先用快照，设置页改动不再影响本任务。
        let snapshot = steps_snapshot_of(store.workflow());
        task.set_steps_snapshot(&snapshot).map_err(orc_error)?;
        task.mark_started().map_err(orc_error)?;
        store.save(task.clone()).await.map_err(orc_error)?;
        // 开始即派活第 1 步（新会话开工，open=true）：失败只标记 blocked（§4.6 不自动重推）
        // + 呈现层推失败提醒，不影响已落库的开始结果与命令返回。
        self.dispatch_step(&store, &task).await;
        let task = store.get_task(&payload.task_id).await.map_err(orc_error)?;
        orc_task_to_dto(&task, store.workflow())
    }

    /// 继续迭代（人工，§4 循环）：本轮结束后开始新一轮——轮次 +1、回到第 1 步（项目经理重新规划），
    /// 可带「本轮要求」（留空则由项目经理按上一轮结论继续）。运行中的任务拒绝（等本轮结束再继续）。
    pub async fn continue_task(
        &self,
        payload: ContinueOrcTaskPayload,
    ) -> Result<OrcTaskDto, CommandError> {
        let _guard = self.task_lock(&payload.task_id).await;
        let (store, mut task) = self.task_store(&payload.task_id).await?;
        match task.state() {
            TaskState::Completed
            | TaskState::Failed
            | TaskState::Canceled
            | TaskState::Rejected => {}
            _ => {
                return Err(CommandError::new(
                    "orc_task_running",
                    "任务还在进行中：等本轮结束后再点「继续迭代」",
                ));
            }
        }
        let instruction = payload
            .instruction
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty());
        task.begin_round(instruction).map_err(orc_error)?;
        store.save(task.clone()).await.map_err(orc_error)?;
        let task = store.get_task(&payload.task_id).await.map_err(orc_error)?;
        self.dispatch_step(&store, &task).await;
        let task = store.get_task(&payload.task_id).await.map_err(orc_error)?;
        orc_task_to_dto(&task, store.workflow())
    }

    /// 全局默认通知节奏：已装配呈现层时读设置（缺失/非法回退 final_only 并告警）；
    /// 未装配（向前兼容）按 final_only。
    async fn global_default_notify_mode(&self) -> NotifyMode {
        match &self.presenter {
            Some(presenter) => presenter.default_notify_mode().await,
            None => NotifyMode::FinalOnly,
        }
    }

    pub async fn list(&self) -> Result<Vec<OrcTaskDto>, CommandError> {
        let repository = self.repository().await?;
        let node_config = self.node_config().await;
        let tasks = repository.list_tasks().await.map_err(repository_error)?;
        let mut result = Vec::with_capacity(tasks.len());
        for raw in tasks {
            let task = OrcTask::from_a2a(raw).map_err(orc_error)?;
            let workflow = self.resolve_task_workflow(&task.workflow_id().map_err(orc_error)?)?;
            // 有快照（start 已锁定）→ 快照；无快照（未开始/旧任务）→ 实时合并。
            let effective = match task.steps_snapshot().map_err(orc_error)? {
                Some(snapshot) if !snapshot.is_empty() => {
                    apply_steps_snapshot(&workflow, &snapshot)
                }
                _ => node_config.merge_workflow(&workflow),
            };
            result.push(orc_task_to_dto(&task, &effective)?);
        }
        Ok(result)
    }

    /// 当前编排工作流（旧 UI 创建任务前预览：预置选择形态）。新 UI 用 `list_orc_templates`。
    pub async fn current_workflow(&self) -> Result<CurrentOrcWorkflowDto, CommandError> {
        let store = self.resolve_legacy_store().await?;
        Ok(CurrentOrcWorkflowDto {
            workflow: orc_workflow_to_dto(store.workflow()),
        })
    }

    /// 固定模板列表（设置页节点配置与创建任务预览共用）：节点含合并后的 Agent/模型。
    pub async fn list_orc_templates(&self) -> Result<Vec<OrcTemplateDto>, CommandError> {
        let node_config = self.node_config().await;
        Ok(template_dtos(&node_config))
    }

    /// 保存某模板的节点配置（覆盖式，§3）：校验通过后返回保存后的全量模板列表。
    pub async fn save_orc_template_config(
        &self,
        payload: SaveOrcTemplateConfigPayload,
    ) -> Result<Vec<OrcTemplateDto>, CommandError> {
        let Some(settings) = &self.settings else {
            return Err(CommandError::new(
                ORC_SETTINGS_UNAVAILABLE,
                "编排设置存储不可用：无法保存节点配置",
            ));
        };
        let template_id = payload.template_id.trim();
        let workflow = Workflow::builtin(template_id).ok_or_else(|| {
            CommandError::new(
                ORC_TEMPLATE_UNKNOWN,
                format!("模板不存在：{template_id}（可选：快速修复 / 标准交付 / 完整评估）"),
            )
        })?;
        let entries = validate_template_steps(&workflow, &payload.steps)?;
        let mut node_config = self.node_config().await;
        node_config.set_template(template_id, entries);
        let mut values = BTreeMap::new();
        values.insert(
            KEY_ORCHESTRATION_NODE_CONFIG.to_string(),
            node_config.to_json(),
        );
        settings
            .store()
            .write_settings_entries(values)
            .await
            .map_err(|error| {
                CommandError::new(
                    "settings_write_failed",
                    format!("保存节点配置失败：{error}"),
                )
            })?;
        Ok(template_dtos(&node_config))
    }

    /// OpenCode 已知项目（工作目录下拉数据源，§3.1）：失败明确报错（界面退回手动输入）。
    pub async fn list_opencode_projects(&self) -> Result<Vec<OpencodeProjectDto>, CommandError> {
        super::opencode_projects::list_projects().await
    }

    /// OpenCode 可用模型（模型下拉数据源）：只读本地服务 API；失败明确报错退回手动输入。
    pub async fn list_opencode_models(&self) -> Result<Vec<OpencodeModelDto>, CommandError> {
        self.list_models().await
    }

    /// 模型目录读取：注入实现优先，未注入回退本机 OpenCode（行为与旧版一致，B2）。
    async fn list_models(&self) -> Result<Vec<OpencodeModelDto>, CommandError> {
        match &self.model_catalog {
            Some(catalog) => catalog.list_models().await,
            None => super::opencode_models::list_models().await,
        }
    }

    /// 旧 `get_current_orc_workflow` 的装配解析（动态模式沿用 `orchestration.workflow` 设置）。
    async fn resolve_legacy_store(&self) -> Result<OrcStore, CommandError> {
        let disabled =
            || CommandError::new(ORCHESTRATION_DISABLED_CODE, ORCHESTRATION_DISABLED_MESSAGE);
        if let Some(store) = &self.store {
            return Ok(store.clone());
        }
        let (Some(sqlite), Some(settings)) = (&self.sqlite, &self.settings) else {
            return Err(disabled());
        };
        let (enabled, workflow_name) = match settings.store().settings_entries().await {
            Ok(entries) => {
                let enabled = entries
                    .get(KEY_ORCHESTRATION_ENABLED)
                    .and_then(serde_json::Value::as_bool)
                    .unwrap_or(true); // 默认开启（体验修正）。
                let workflow = entries
                    .get(KEY_ORCHESTRATION_WORKFLOW)
                    .and_then(serde_json::Value::as_str)
                    .map(str::trim)
                    .unwrap_or("")
                    .to_owned();
                (enabled, workflow)
            }
            Err(error) => {
                tracing::warn!(%error, "读取编排设置失败，按默认开启 + 默认工作流处理");
                (true, String::new())
            }
        };
        if !enabled {
            return Err(disabled());
        }
        let workflow = match workflow_name.as_str() {
            "opencode-only" => Workflow::preset_opencode_only(),
            _ => Workflow::preset(false),
        }
        .expect("预置工作流必须有效");
        Ok(OrcStore::with_repository(workflow, sqlite.clone()))
    }

    pub async fn advance(
        &self,
        payload: AdvanceOrcTaskPayload,
    ) -> Result<OrcTaskDto, CommandError> {
        let _guard = self.task_lock(&payload.task_id).await;
        self.advance_locked(payload).await
    }

    /// 无锁内核：`report_from_agent` 持有同一把任务锁时直接调用（避免自锁死）。
    async fn advance_locked(
        &self,
        payload: AdvanceOrcTaskPayload,
    ) -> Result<OrcTaskDto, CommandError> {
        let (store, current) = self.task_store(&payload.task_id).await?;
        // 未开始的任务不接受推进（先点「开始执行」；也覆盖微信侧对未开始任务的指令）。
        if !current.is_started().map_err(orc_error)? {
            return Err(CommandError::new(
                "orc_task_not_started",
                "任务尚未开始执行：请先在集群页点「开始执行」",
            ));
        }
        // 汇总阶段只等首节点会话上报（report_from_agent）；人工推进明确拒绝（不猜测）。
        if current.is_finalizing().map_err(orc_error)? {
            return Err(CommandError::new(
                ORC_TASK_FINALIZING,
                "任务正在等待项目经理汇总，无需手动推进",
            ));
        }
        let kind = parse_message_kind(payload.kind);
        let order = current.current_step().map_err(orc_error)?;
        let step = store
            .workflow()
            .step(order)
            .ok_or_else(|| orc_error(OrcError::step_not_found(order)))?
            .clone();
        let workflow = store.workflow().clone();
        // 最后一步完成（无门 Report / 通过确认门）→ 进入「项目经理汇总阶段」而非直接完成（§4）。
        if completes_last_step(kind, current.state(), &step, &workflow) {
            let task = store
                .enter_finalizing(&payload.task_id)
                .await
                .map_err(orc_error)?;
            let dto = orc_task_to_dto(&task, &workflow)?;
            if let Some(presenter) = &self.presenter {
                self.present_finalizing(presenter, &workflow, &task, order)
                    .await;
            }
            self.dispatch_summary(&store, &task).await;
            return Ok(dto);
        }
        let outcome = store
            .on_message(&payload.task_id, kind)
            .await
            .map_err(orc_error)?;
        let task = store.get_task(&payload.task_id).await.map_err(orc_error)?;
        let dto = orc_task_to_dto(&task, &workflow)?;
        // P1-4 呈现层单一入口：推进后按任务通知节奏决定是否外发微信（失败不阻塞命令结果）。
        if let Some(presenter) = &self.presenter {
            self.present_advance(presenter, &workflow, &task, &dto, kind, &outcome)
                .await;
        }
        // P2 派活：Advance/BackToWork/Recover 且任务未完成 → 把当前目标 Step 的信封
        // 交给该步配置的 Agent（失败只标记 blocked，不改变已落库的推进结果）。
        self.dispatch_current_step(&store, &task, &outcome).await;
        Ok(dto)
    }

    /// Agent 汇报自动回注（§4.4「该 Step 的 Agent 汇报到达」）：
    /// 由 [`super::orc_report_observer::OrcReportObserver`] 在 session 完成事件匹配
    /// `task-<id>-step-<n>` 时调用——校验任务正处于第 n 步且干活中，记录产出（`body`）
    /// 后复用 [`Self::advance`]（Report）完成推进（含通知节奏呈现与下一步派活），
    /// 保证与人工推进同一条链路。
    ///
    /// 失败语义（§4.6）：`failed=true`（插件显式标记，或旧插件正文以「任务执行失败：」开头）
    /// **不 advance**——普通步骤 → blocked（「Step N 执行失败：…」）+ 失败提醒；
    /// 汇总阶段首节点失败 → blocked（「项目经理汇总失败：…」）。成功路径不变。
    ///
    /// 汇总阶段（§4）：首节点（step=1）会话的成功上报 = 最终汇报 → 任务 Completed 并推送呈现层。
    ///
    /// 返回 `Ok(false)` = 过期/无关事件（任务不存在、步不一致、任务非干活中），静默忽略；
    /// 错误 = 回注自身失败（由调用方记日志，不影响事件消费）。
    ///
    /// `settled_turn_ms`：看门狗回注时传入本回合完成时刻；命中（`Ok(true)`）时**在同一把
    /// 任务锁内**记录结算时刻，消除「回注后另起一次读改写整任务覆盖」的丢更新窗口（B1）。
    /// 插件回注路径传 `None`，行为与旧版一致。
    pub async fn report_from_agent(
        &self,
        task_id: &str,
        step: u32,
        body: &str,
        failed: bool,
        settled_turn_ms: Option<i64>,
    ) -> Result<bool, CommandError> {
        let _guard = self.task_lock(task_id).await;
        let applied = self
            .report_from_agent_locked(task_id, step, body, failed)
            .await?;
        if applied {
            if let Some(completed_at_ms) = settled_turn_ms {
                self.mark_settled_turn_locked(task_id, completed_at_ms)
                    .await;
            }
        }
        Ok(applied)
    }

    /// 无锁内核：调用方必须已持有该任务的任务锁（`advance_locked` 同理由此直调）。
    async fn report_from_agent_locked(
        &self,
        task_id: &str,
        step: u32,
        body: &str,
        failed: bool,
    ) -> Result<bool, CommandError> {
        let (store, task) = match self.task_store(task_id).await {
            Ok(resolved) => resolved,
            Err(error) if error.code() == ORC_TASK_NOT_FOUND_CODE => return Ok(false),
            Err(error) => return Err(error),
        };
        if !task.is_started().unwrap_or(false) {
            return Ok(false);
        }
        // 显式失败标记优先；旧插件（无标记）按正文前缀识别，保持兼容。
        let failed = failed || legacy_failure_marker(body);
        if task.is_finalizing().unwrap_or(false) {
            // 汇总阶段只认干活中的首节点会话；其它步/阻塞中的迟到事件忽略。
            if step != 1 || task.state() != TaskState::Working {
                return Ok(false);
            }
            if failed {
                self.auto_blocked(
                    &store,
                    task_id,
                    1,
                    format!("{SUMMARY_ROUND_FAILED_PREFIX}{}", failure_detail(body)),
                )
                .await;
                return Ok(true);
            }
            // 项目经理判定「继续迭代」且未达轮次上限 → 不完成：自动开始新一轮（§4 迭代循环）。
            if let Some(input) = round_continue_input(body) {
                let round = task.round().unwrap_or(1);
                if round < ORC_MAX_ROUNDS {
                    let mut next = store.get_task(task_id).await.map_err(orc_error)?;
                    // 本轮结论摘要进轮次时间线；随后 begin_round 追加新一轮记录。
                    next.record_round_summary(body).map_err(orc_error)?;
                    next.begin_round(Some(input.as_str())).map_err(orc_error)?;
                    store.save(next.clone()).await.map_err(orc_error)?;
                    self.present_round_continue(&store, &next, round, &input)
                        .await;
                    self.dispatch_step(&store, &next).await;
                    return Ok(true);
                }
                tracing::warn!(
                    task_id,
                    round,
                    "项目经理判定继续迭代但已达轮次上限，按完成处理（用户可手动继续迭代）"
                );
            }
            // 本轮结论摘要进轮次时间线（完成任务也保留每轮结论）。
            let mut finished = store.get_task(task_id).await.map_err(orc_error)?;
            finished.record_round_summary(body).map_err(orc_error)?;
            store.save(finished).await.map_err(orc_error)?;
            let task = store
                .complete_finalizing(task_id)
                .await
                .map_err(orc_error)?;
            if let Some(presenter) = &self.presenter {
                self.present_final(presenter, store.workflow(), &task, body)
                    .await;
            }
            return Ok(true);
        }
        if task.current_step().ok() != Some(step) || task.state() != TaskState::Working {
            return Ok(false);
        }
        // 失败回合：不记录产出、不推进，直接 blocked + 失败提醒（与人工 mark_blocked 同链路）。
        if failed {
            self.auto_blocked(
                &store,
                task_id,
                step,
                format!("第 {step} 步执行失败：{}", failure_detail(body)),
            )
            .await;
            return Ok(true);
        }
        // 推进前记录该步产出（回流汇总用，单步/总量截断由 orchestration 负责）。
        store
            .record_step_report(task_id, step, body)
            .await
            .map_err(orc_error)?;
        self.advance_locked(AdvanceOrcTaskPayload {
            task_id: task_id.to_owned(),
            kind: OrcMessageKindDto::Report,
        })
        .await?;
        Ok(true)
    }

    /// P1-4 推进后呈现：按 `should_notify` 规则决定是否外发微信集群消息（§4.6）。
    /// 元数据损坏等呈现侧失败只告警，不影响已落库的推进结果。
    async fn present_advance(
        &self,
        presenter: &Arc<dyn OrcClusterPresenter>,
        workflow: &Workflow,
        task: &OrcTask,
        dto: &OrcTaskDto,
        kind: MessageKind,
        outcome: &StepOutcome,
    ) {
        let mode = match task.notify_mode() {
            Ok(mode) => mode,
            Err(error) => {
                tracing::warn!(
                    task_id = %task.id(),
                    code = error.code.as_str(),
                    "读取任务通知节奏失败，跳过集群消息推送"
                );
                return;
            }
        };
        if !should_notify(mode, outcome) {
            return;
        }
        let total = workflow.max_order();
        let step = dto.current_step;
        // 正文里的"收到汇报的那一步"：Advance 后 current_step 已指向下一步，需回退一步。
        let reported_step = if outcome.action == TransitionAction::Advance {
            step.saturating_sub(1)
        } else {
            step
        };
        let body = progress_body(kind, outcome.action, reported_step);
        let text = render_cluster_message(
            &cluster_task_label(task),
            step,
            total,
            state_cn(&dto.state),
            &body,
        );
        presenter.push(task.id(), text).await;
    }

    /// 「进入项目经理汇总」呈现（§4）：verbose 推进度；final_only 不推（只推最终汇报）。
    async fn present_finalizing(
        &self,
        presenter: &Arc<dyn OrcClusterPresenter>,
        workflow: &Workflow,
        task: &OrcTask,
        order: u32,
    ) {
        let mode = match task.notify_mode() {
            Ok(mode) => mode,
            Err(error) => {
                tracing::warn!(
                    task_id = %task.id(),
                    code = error.code.as_str(),
                    "读取任务通知节奏失败，跳过汇总进度推送"
                );
                return;
            }
        };
        if mode != NotifyMode::Verbose {
            return;
        }
        let text = render_cluster_message(
            &cluster_task_label(task),
            order,
            workflow.max_order(),
            state_cn(&OrcTaskStateDto::Working),
            &finalizing_body(order),
        );
        presenter.push(task.id(), text).await;
    }

    /// 最终项目经理汇报呈现（§4）：正文 = 首节点产出截断（1200 字），
    /// final_only/verbose 都要推这一条（失败提醒不受限的既有语义不变）。
    async fn present_final(
        &self,
        presenter: &Arc<dyn OrcClusterPresenter>,
        workflow: &Workflow,
        task: &OrcTask,
        body: &str,
    ) {
        let total = workflow.max_order();
        let step = task.current_step().unwrap_or(total).min(total.max(1));
        let text = render_cluster_message(
            &cluster_task_label(task),
            step,
            total,
            state_cn(&OrcTaskStateDto::Completed),
            &truncate_final_report(body),
        );
        presenter.push(task.id(), text).await;
    }

    /// 自动继续迭代的进度呈现：仅 verbose 模式推（final_only 只推最终汇报，§5）。
    async fn present_round_continue(
        &self,
        store: &OrcStore,
        task: &OrcTask,
        round: u32,
        input: &str,
    ) {
        let Some(presenter) = &self.presenter else {
            return;
        };
        let mode = match task.notify_mode() {
            Ok(mode) => mode,
            Err(_) => return,
        };
        if mode != NotifyMode::Verbose {
            return;
        }
        let detail = input.trim();
        let detail = if detail.is_empty() {
            "按上一轮结论继续".to_string()
        } else {
            let mut text: String = detail.chars().take(80).collect();
            if detail.chars().count() > 80 {
                text.push('…');
            }
            text
        };
        let total = store.workflow().max_order();
        let text = render_cluster_message(
            &cluster_task_label(task),
            1,
            total,
            state_cn(&OrcTaskStateDto::Working),
            &format!(
                "第 {round} 轮完成：项目经理判定继续迭代，已自动开始第 {} 轮（{detail}）",
                round + 1
            ),
        );
        presenter.push(task.id(), text).await;
    }

    pub async fn mark_blocked(
        &self,
        payload: MarkBlockedOrcTaskPayload,
    ) -> Result<OrcTaskDto, CommandError> {
        let _guard = self.task_lock(&payload.task_id).await;
        let (store, _task) = self.task_store(&payload.task_id).await?;
        let reason = payload.reason.trim();
        if reason.is_empty() {
            return Err(CommandError::new(
                "orc_block_reason_empty",
                "阻塞原因不能为空（请写清哪一步失败、谁不可用、未送达）",
            ));
        }
        let task = store
            .mark_blocked(&payload.task_id, payload.step, reason)
            .await
            .map_err(orc_error)?;
        let dto = orc_task_to_dto(&task, store.workflow())?;
        // P1-4 失败提醒不受 notify_mode 限制：一律外发（§4.6：写清失败 Step/原因，不自动重推）。
        if let Some(presenter) = &self.presenter {
            self.present_blocked(presenter, store.workflow(), &task, payload.step, reason)
                .await;
        }
        Ok(dto)
    }

    /// P1-4 失败提醒呈现：写清哪一步失败、原因与处理办法（会话语义与命令 mark_blocked 一致）。
    async fn present_blocked(
        &self,
        presenter: &Arc<dyn OrcClusterPresenter>,
        workflow: &Workflow,
        task: &OrcTask,
        step: u32,
        reason: &str,
    ) {
        let total = workflow.max_order();
        let text = render_cluster_message(
            &cluster_task_label(task),
            step,
            total,
            state_cn(&OrcTaskStateDto::Failed),
            &failure_body(step, reason),
        );
        presenter.push(task.id(), text).await;
    }

    /// P2 推进后自动派活：`outcome.action` 属于要干活的转移（Advance/BackToWork/Recover）
    /// 且任务未完成时，派活当前目标 Step 的 Agent；其余转移（Stay/WaitConfirm/Complete）不派活。
    ///
    /// 与呈现层同语义：派活成功/失败都不改变已落库的推进结果；失败只标记 blocked
    /// （§4.6 不自动重推），由 [`Self::present_blocked`] 推微信失败提醒，不阻塞命令返回。
    async fn dispatch_current_step(&self, store: &OrcStore, task: &OrcTask, outcome: &StepOutcome) {
        use TransitionAction::{Advance, BackToWork, Recover};
        if !matches!(outcome.action, Advance | BackToWork | Recover) {
            return;
        }
        if task.state() == TaskState::Completed {
            return;
        }
        self.dispatch_step(store, task).await;
    }

    /// P2 派活当前步骤：组装信封 → 交给该步配置的 Agent（Step 1 试图开新会话，后续步续聊）。
    ///
    /// - 未注入 driver → 保持 P1-3/1-4 行为（只推进 + 呈现，不派活，agent_hint 缺失也不报错）；
    /// - 步骤缺失（内部不一致）→ 记 error 日志后跳过，不阻塞推进、不标记阻塞；
    /// - `agent_hint` 缺失 → 明确错误码 `orc_step_agent_missing` 并自动 blocked
    ///   （写清「Step N 未配置 Agent」，用户可改配置后恢复）。
    ///
    /// §4 派活透传：工作目录（任务级）、该步模型（合并后）、无人值守（settings）随信封下发。
    async fn dispatch_step(&self, store: &OrcStore, task: &OrcTask) {
        // 未注入 driver：保持 P1-3/1-4 行为（推进 + 呈现，不派活）。
        let Some(driver) = self.driver.as_ref() else {
            return;
        };
        let current_step = match task.current_step() {
            Ok(step) => step,
            Err(error) => {
                tracing::error!(
                    task_id = %task.id(),
                    code = error.code.as_str(),
                    "读取任务当前步骤失败，跳过派活"
                );
                return;
            }
        };
        let Some(step) = store.workflow().step(current_step).cloned() else {
            tracing::error!(
                task_id = %task.id(),
                step = current_step,
                "工作流缺少当前步骤（内部不一致），跳过派活"
            );
            return;
        };
        let Some(agent_hint) = step.agent_hint.as_deref() else {
            let reason = format!("Step {current_step} 未配置 Agent（agent_hint），无法派活");
            tracing::warn!(task_id = %task.id(), step = current_step, code = ORC_STEP_AGENT_MISSING, "{reason}");
            self.auto_blocked(store, task.id(), current_step, reason)
                .await;
            return;
        };
        let goal = match task.goal() {
            Ok(goal) => goal,
            Err(error) => {
                tracing::error!(
                    task_id = %task.id(),
                    code = error.code.as_str(),
                    "读取任务目标失败，跳过派活"
                );
                return;
            }
        };
        let working_dir = match task.working_dir() {
            Ok(dir) => dir,
            Err(error) => {
                tracing::error!(
                    task_id = %task.id(),
                    code = error.code.as_str(),
                    "读取任务工作目录失败，跳过派活"
                );
                return;
            }
        };
        // 会话标题（用户可见）：任务短名 + 步骤；读不到名称时按目标前 8 字推导。
        let session_title = match task.name() {
            Ok(name) => display_name(name.as_deref(), &goal),
            Err(error) => {
                tracing::warn!(
                    task_id = %task.id(),
                    code = error.code.as_str(),
                    "读取任务名称失败，会话标题按目标推导"
                );
                display_name(None, &goal)
            }
        };
        // 信封渲染（用户模板优先、按角色的内置默认兜底，§4.3 / P1-5）：渲染告警只记日志不阻断。
        let next_role = store
            .workflow()
            .next_step(current_step)
            .map(|next| next.role.as_str());
        let round = task.round().unwrap_or(1);
        let rendered =
            self.templates
                .render_envelope(store.workflow(), &step, &goal, next_role, round);
        // 第 2 轮起：把本轮要求/上一轮结论作为前缀交给项目经理（首步续聊有上下文）。
        let rendered_text = if current_step == 1 && round > 1 {
            let input = task.round_input().unwrap_or(None);
            format!("{}{}", round_intro(round, input.as_deref()), rendered.text)
        } else {
            rendered.text.clone()
        };
        for warning in &rendered.warnings {
            tracing::warn!(
                task_id = %task.id(),
                kind = %warning.kind.as_str(),
                "{}",
                warning.message
            );
        }
        let targets = match dispatch_targets(task.id(), agent_hint, current_step) {
            Ok(targets) => targets,
            Err(reason) => {
                tracing::warn!(task_id = %task.id(), step = current_step, "{reason}");
                self.auto_blocked(store, task.id(), current_step, reason)
                    .await;
                return;
            }
        };
        let (agent_id, session_id) = targets;
        let open = current_step == 1;
        let options = DispatchOptions {
            working_dir,
            model: step.model.clone(),
            variant: step.variant.clone(),
            unattended: self.unattended().await,
            title: Some(format!("【集群】{session_title} · 第 {current_step} 步")),
        };
        // 记录派活时刻（宿主看门狗用：只认此后的回合产出，避免把上一轮正文当成本次汇报）。
        let dispatched_at_ms = Timestamp::now_utc().unix_millis();
        match store.get_task(task.id()).await {
            Ok(mut dispatched) => {
                if let Err(error) = dispatched.mark_dispatched(dispatched_at_ms) {
                    tracing::warn!(task_id = %task.id(), code = error.code.as_str(), "记录派活时刻失败（看门狗将跳过本步）");
                } else if let Err(error) = store.save(dispatched).await {
                    tracing::warn!(task_id = %task.id(), "保存派活时刻失败（看门狗将跳过本步）：{error}");
                }
            }
            Err(error) => {
                tracing::warn!(task_id = %task.id(), code = error.code.as_str(), "读取任务失败，无法记录派活时刻（看门狗将跳过本步）");
            }
        }
        if let Err(error) = driver
            .dispatch(
                task.id(),
                &agent_id,
                &session_id,
                &rendered_text,
                open,
                &options,
            )
            .await
        {
            tracing::warn!(
                task_id = %task.id(),
                step = current_step,
                code = error.code(),
                "派活失败：{}",
                error.message()
            );
            self.auto_blocked(store, task.id(), current_step, error.message().to_string())
                .await;
        }
    }

    /// 派活「汇总信封」到首节点会话（resume，§4）：任务处于 finalizing 时使用，
    /// 也用于 blocked 后自动重派（v2 修订：不再需要再点一次「发指令」）。
    ///
    /// 失败 → blocked（blocked_step = 1，原因「最终汇总：…」）+ 失败提醒。
    async fn dispatch_summary(&self, store: &OrcStore, task: &OrcTask) {
        let Some(driver) = self.driver.as_ref() else {
            return;
        };
        let workflow = store.workflow();
        let Some(first) = workflow.step(1).cloned() else {
            tracing::error!(task_id = %task.id(), "工作流缺少首节点，跳过汇总派活");
            return;
        };
        let Some(agent_hint) = first.agent_hint.as_deref() else {
            let reason = "第 1 步（项目经理）未配置 Agent，无法派活汇总汇报".to_string();
            tracing::warn!(task_id = %task.id(), code = ORC_STEP_AGENT_MISSING, "{reason}");
            self.auto_blocked(store, task.id(), 1, reason).await;
            return;
        };
        let goal = match task.goal() {
            Ok(goal) => goal,
            Err(error) => {
                tracing::error!(
                    task_id = %task.id(),
                    code = error.code.as_str(),
                    "读取任务目标失败，跳过汇总派活"
                );
                return;
            }
        };
        let working_dir = match task.working_dir() {
            Ok(dir) => dir,
            Err(error) => {
                tracing::error!(
                    task_id = %task.id(),
                    code = error.code.as_str(),
                    "读取任务工作目录失败，跳过汇总派活"
                );
                return;
            }
        };
        let reports = match task.meta() {
            Ok(meta) => render_step_reports(workflow, &meta.step_reports),
            Err(error) => {
                tracing::error!(
                    task_id = %task.id(),
                    code = error.code.as_str(),
                    "读取任务产出失败，跳过汇总派活"
                );
                return;
            }
        };
        let rendered = self.templates.render_summary_envelope(
            workflow,
            &goal,
            &reports,
            task.round().unwrap_or(1),
        );
        for warning in &rendered.warnings {
            tracing::warn!(
                task_id = %task.id(),
                kind = %warning.kind.as_str(),
                "{}",
                warning.message
            );
        }
        let targets = match dispatch_targets(task.id(), agent_hint, 1) {
            Ok(targets) => targets,
            Err(reason) => {
                tracing::warn!(task_id = %task.id(), "{reason}");
                self.auto_blocked(store, task.id(), 1, reason).await;
                return;
            }
        };
        let (agent_id, session_id) = targets;
        // 汇总复用首步会话（resume 不新建会话），标题保持一致语义、不影响既有会话标题。
        let session_title = match task.name() {
            Ok(name) => display_name(name.as_deref(), &goal),
            Err(_) => display_name(None, &goal),
        };
        let options = DispatchOptions {
            working_dir,
            model: first.model.clone(),
            variant: first.variant.clone(),
            unattended: self.unattended().await,
            title: Some(format!("【集群】{session_title} · 汇总汇报")),
        };
        if let Err(error) = driver
            .dispatch(
                task.id(),
                &agent_id,
                &session_id,
                &rendered.text,
                false,
                &options,
            )
            .await
        {
            tracing::warn!(
                task_id = %task.id(),
                step = 1,
                code = error.code(),
                "汇总汇报派活失败：{}",
                error.message()
            );
            self.auto_blocked(
                store,
                task.id(),
                1,
                format!("{SUMMARY_DISPATCH_FAILED_PREFIX}{}", error.message()),
            )
            .await;
        }
    }

    /// P2 派活失败 → 自动 blocked（§4.6：不自动重推，等人工处理）并推微信失败提醒。
    /// 落库失败（如任务已终止）只记日志，不再改变推进结果。
    async fn auto_blocked(&self, store: &OrcStore, task_id: &str, step: u32, reason: String) {
        match store.mark_blocked(task_id, step, &reason).await {
            Ok(task) => {
                if let Some(presenter) = &self.presenter {
                    self.present_blocked(presenter, store.workflow(), &task, step, &reason)
                        .await;
                }
            }
            Err(error) => {
                tracing::error!(
                    task_id,
                    step,
                    code = error.code.as_str(),
                    "派活失败后自动标记阻塞失败：{}",
                    error.message
                );
            }
        }
    }

    /// blocked → 用户重新发起（桌面端/微信）：清阻塞后**自动重新派活**（v2 修订）。
    /// 处于汇总阶段时重派「汇总信封」，否则重派当前步骤普通信封。
    pub async fn recover_blocked(
        &self,
        payload: OrcTaskIdPayload,
    ) -> Result<OrcTaskDto, CommandError> {
        let _guard = self.task_lock(&payload.task_id).await;
        let (store, _task) = self.task_store(&payload.task_id).await?;
        let task = store
            .recover_blocked(&payload.task_id)
            .await
            .map_err(orc_error)?;
        let dto = orc_task_to_dto(&task, store.workflow())?;
        if task.is_finalizing().unwrap_or(false) {
            self.dispatch_summary(&store, &task).await;
        } else {
            self.dispatch_step(&store, &task).await;
        }
        Ok(dto)
    }
}

/// 装配编排时加载用户 harness 模板配置（§4.3 / P1-5）。
///
/// 文件缺失 = 正常未配置（内置默认兜底，不告警）；读取/解析/单条损坏 →
/// 每条告警写清哪里失败、如何回退并记入日志（不静默），编排不会因用户配置坏而瘫痪。
pub fn load_harness_templates(config_dir: impl AsRef<Path>) -> TemplateResolver {
    let result =
        TemplateResolver::from_config_file(config_dir.as_ref().join(HARNESS_TEMPLATES_FILE));
    for warning in &result.warnings {
        tracing::warn!(
            kind = %warning.kind.as_str(),
            "{}",
            warning.message
        );
    }
    result.resolver
}

/// 通知节奏解析：缺省 final_only（只推最终汇报，§4.6）；未知取值明确报错，不猜测。
fn parse_notify_mode(raw: Option<&str>) -> Result<NotifyMode, CommandError> {
    match raw {
        None | Some("final_only") => Ok(NotifyMode::FinalOnly),
        Some("verbose") => Ok(NotifyMode::Verbose),
        Some(other) => Err(CommandError::new(
            "orc_notify_mode_invalid",
            format!("无效的通知节奏：{other}（可选 final_only / verbose）"),
        )),
    }
}

fn parse_message_kind(kind: OrcMessageKindDto) -> MessageKind {
    match kind {
        OrcMessageKindDto::Report => MessageKind::Report,
        OrcMessageKindDto::Instruction => MessageKind::Instruction,
        OrcMessageKindDto::Confirm => MessageKind::Confirm,
        OrcMessageKindDto::Question => MessageKind::Question,
        OrcMessageKindDto::Info => MessageKind::Info,
    }
}

/// 编排错误 → 命令错误：保留稳定错误码与中文用户消息。
fn orc_error(error: OrcError) -> CommandError {
    CommandError::new(error.code.as_str(), error.message)
}

/// 仓储错误 → 命令错误：保留仓储层稳定错误码与中文消息。
fn repository_error(error: OrcRepositoryError) -> CommandError {
    CommandError::new(error.code(), error.message())
}

/// 本次消息是否「完成最后一步」（§4：完成不直接落 Completed，先进入汇总阶段）：
/// - 无确认门步骤收到汇报 → 完成；
/// - 有确认门步骤在等待确认时收到确认 → 完成（Confirm 通过确认门）。
fn completes_last_step(
    kind: MessageKind,
    state: TaskState,
    step: &agentnotify_orchestration::WorkflowStep,
    workflow: &Workflow,
) -> bool {
    if !workflow.is_last(step.order) {
        return false;
    }
    match kind {
        MessageKind::Report => !step.human_gate,
        MessageKind::Confirm => step.human_gate && state == TaskState::InputRequired,
        _ => false,
    }
}

/// 旧插件兼容：失败终态同样上报为 `session.completed` 且正文以 `任务执行失败：` 开头；
/// 新插件带显式 `failed: true` 标记（两条路都能识别，旧插件不至于把失败当完成推进）。
fn legacy_failure_marker(body: &str) -> bool {
    body.trim().starts_with(AGENT_FAILURE_BODY_PREFIX)
}

/// 失败详情：去空白并剥掉旧插件的「任务执行失败：」前缀；空正文给可读兜底（不猜测细节）。
/// 失败详情：去空白、剥掉旧插件的「任务执行失败：」前缀，并把常见英文错误翻译成
/// 用户看得懂、知道怎么解决的中文一句话（未识别的错误保留原文，只做长度截断）。
fn failure_detail(body: &str) -> String {
    let detail = body.trim();
    let detail = detail
        .strip_prefix(AGENT_FAILURE_BODY_PREFIX)
        .unwrap_or(detail)
        .trim();
    if detail.is_empty() {
        return "Agent 未返回具体错误信息：请打开对应会话查看原因后点「重新发起」".to_string();
    }
    user_facing_failure(detail)
}

/// 阻塞原因展示上限（字符数）：一句话讲清原因与处理办法，超长截断。
const FAILURE_DETAIL_LIMIT: usize = 120;

/// 常见失败 → 一句用户可读的话（含处理建议）：只识别有把握的模型/额度错误；
/// 其余保留原文（不硬翻译、不加戏），超长截断。
fn user_facing_failure(detail: &str) -> String {
    let lower = detail.to_lowercase();
    if lower.contains("insufficient account funds")
        || lower.contains("insufficient funds")
        || lower.contains("quota")
    {
        return "模型服务余额不足：请充值，或给该节点换一个模型后点「重新发起」".to_string();
    }
    if lower.contains("model unavailable")
        || lower.contains("model not found")
        || lower.contains("unknown model")
    {
        return "所选模型不可用：请给该节点换一个模型后点「重新发起」".to_string();
    }
    let trimmed = detail.trim();
    if trimmed.chars().count() <= FAILURE_DETAIL_LIMIT {
        return trimmed.to_string();
    }
    let mut truncated: String = trimmed.chars().take(FAILURE_DETAIL_LIMIT).collect();
    truncated.push('…');
    truncated
}

/// P2 派活目标：由 `agent_hint` 解析 Agent id，并为 (task, step) 生成稳定会话 id
/// （`task-<task_id>-step-<n>`）。Step 1 用该会话开新会话，后续步 resume 同一会话。
/// 解析失败返回用户可读中文原因（不猜测兜底）。
fn dispatch_targets(
    task_id: &str,
    agent_hint: &str,
    step: u32,
) -> Result<(AgentId, AgentSessionId), String> {
    let agent_id = AgentId::new(agent_hint.to_string())
        .map_err(|_| format!("Step {step} 的 Agent 标识无效（{agent_hint}），无法派活"))?;
    let session_id = AgentSessionId::new(format!(
        "{ORC_DISPATCH_SESSION_PREFIX}-{task_id}-step-{step}"
    ))
    .map_err(|_| format!("Step {step} 的会话标识生成失败，无法派活"))?;
    Ok((agent_id, session_id))
}

/// 脱敏后的任务视图：只暴露任务上下文，不暴露内部元数据细节。
fn orc_task_to_dto(task: &OrcTask, workflow: &Workflow) -> Result<OrcTaskDto, CommandError> {
    let meta = task.meta().map_err(orc_error)?;
    let name = display_name(meta.name.as_deref(), &meta.goal);
    Ok(OrcTaskDto {
        id: task.id().to_string(),
        workflow_id: meta.workflow_id,
        state: orc_task_state_dto(task.state()),
        current_step: meta.current_step,
        started: meta.started,
        workflow: orc_workflow_to_dto(workflow),
        blocked_step: meta.blocked_step,
        block_reason: meta.block_reason,
        notify_mode: meta.notify_mode.as_str().to_string(),
        goal: meta.goal,
        name,
        round: meta.round.max(1),
        round_input: meta.round_input,
        created_at: meta.created_at,
        round_history: meta
            .round_history
            .into_iter()
            .map(|record| OrcRoundRecordDto {
                round: record.round,
                input: record.input,
                summary: record.summary,
            })
            .collect(),
        working_dir: meta.working_dir,
        finalizing: meta.final_report_pending,
    })
}

/// 集群消息里的任务标识（用户可读）：任务名称（缺省按目标推导）；元数据损坏时退回任务 ID 以便定位。
fn cluster_task_label(task: &OrcTask) -> String {
    match task.meta() {
        Ok(meta) => display_name(meta.name.as_deref(), &meta.goal),
        Err(_) => task.id().to_string(),
    }
}

/// 任务展示名：显式名称优先；旧任务缺省时按目标前 8 字推导（界面/会话标题始终可读）。
fn display_name(name: Option<&str>, goal: &str) -> String {
    if let Some(value) = name.map(str::trim).filter(|value| !value.is_empty()) {
        return value.to_string();
    }
    derive_task_name(goal)
}

/// 从目标推导短名（去空白后取前 8 字；调用方保证目标非空）。
fn derive_task_name(goal: &str) -> String {
    goal.trim().chars().take(ORC_TASK_NAME_MAX_CHARS).collect()
}

/// 任务名称校验（必填、≤8 字、禁分隔符）：空/超长/含 `】`、`【` 都明确报错，不截断猜测。
fn validate_task_name(raw: &str) -> Result<String, CommandError> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(CommandError::new(
            ORC_TASK_NAME_INVALID,
            "任务名称不能为空：请填 8 个字以内的短名",
        ));
    }
    // B3：`】`/`【` 会破坏微信指令 `【集群 <名>】` 的寻址，创建/编辑一律拒绝（不猜测清洗）。
    if let Some(found) = trimmed
        .chars()
        .find(|c| ORC_TASK_NAME_FORBIDDEN_CHARS.contains(c))
    {
        return Err(CommandError::new(
            ORC_TASK_NAME_INVALID,
            format!("任务名称不能包含「{found}」：它会干扰微信指令寻址，请改用其他字符"),
        ));
    }
    let count = trimmed.chars().count();
    if count > ORC_TASK_NAME_MAX_CHARS {
        return Err(CommandError::new(
            ORC_TASK_NAME_INVALID,
            format!("任务名称最多 {ORC_TASK_NAME_MAX_CHARS} 个字，当前 {count} 个字：请精简后重试"),
        ));
    }
    Ok(trimmed.to_string())
}

/// 思考强度白名单校验（B2）：模型必须存在，且 variant 在该模型的 `variants` 中；
/// 模型未知 / 不在列表 → 稳定错误码 `orc_step_variant_invalid` + 面向用户的中文提示。
fn validate_variant(
    models: &[OpencodeModelDto],
    model: &str,
    variant: &str,
) -> Result<(), CommandError> {
    let Some(found) = models
        .iter()
        .find(|entry| format!("{}/{}", entry.provider_id, entry.model_id) == model)
    else {
        return Err(CommandError::new(
            ORC_STEP_VARIANT_INVALID,
            format!("未找到模型 {model}：请确认 OpenCode 已打开并重新读取模型列表"),
        ));
    };
    if !found.variants.iter().any(|item| item == variant) {
        let options = if found.variants.is_empty() {
            "该模型没有可选思考强度".to_string()
        } else {
            format!("可选：{}", found.variants.join(" / "))
        };
        return Err(CommandError::new(
            ORC_STEP_VARIANT_INVALID,
            format!(
                "思考强度 {variant} 不适用于模型 {}（{options}）：请从下拉列表重新选择",
                found.name
            ),
        ));
    }
    Ok(())
}

/// 工作流视图：节点列表（角色/建议 Agent/模型/人工确认门）供 UI 预览「每步做什么、派给谁」。
fn orc_workflow_to_dto(workflow: &Workflow) -> OrcWorkflowDto {
    OrcWorkflowDto {
        id: workflow.id.clone(),
        name: workflow.name.clone(),
        steps: workflow
            .steps
            .iter()
            .map(|step| OrcWorkflowStepDto {
                order: step.order,
                role: step.role.clone(),
                agent_hint: step.agent_hint.clone(),
                model: step.model.clone(),
                variant: step.variant.clone(),
                human_gate: step.human_gate,
            })
            .collect(),
    }
}

/// A2A `TaskState` → 稳定 DTO 字符串（§8.3 映射表的桌面呈现侧）。
/// `TaskState` 带 `#[non_exhaustive]`：未来新增状态统一落到 `Unspecified`，保持契约稳定。
fn orc_task_state_dto(state: TaskState) -> OrcTaskStateDto {
    match state {
        TaskState::Unspecified => OrcTaskStateDto::Unspecified,
        TaskState::Submitted => OrcTaskStateDto::Submitted,
        TaskState::Working => OrcTaskStateDto::Working,
        TaskState::Completed => OrcTaskStateDto::Completed,
        TaskState::Failed => OrcTaskStateDto::Failed,
        TaskState::Canceled => OrcTaskStateDto::Canceled,
        TaskState::InputRequired => OrcTaskStateDto::InputRequired,
        TaskState::Rejected => OrcTaskStateDto::Rejected,
        TaskState::AuthRequired => OrcTaskStateDto::AuthRequired,
        _ => OrcTaskStateDto::Unspecified,
    }
}

/// 新一轮的派活前缀（第 2 轮起）：把本轮要求/上一轮结论交给项目经理，保证续聊有上下文。
fn round_intro(round: u32, input: Option<&str>) -> String {
    match input.map(str::trim).filter(|value| !value.is_empty()) {
        Some(text) => format!(
            "【第 {round} 轮迭代】本轮要求（来自用户或上一轮结论）：\n{text}\n────────────────────────\n"
        ),
        None => {
            format!("【第 {round} 轮迭代】沿用上一轮结论继续推进。\n────────────────────────\n")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 新一轮前缀：带要求时写清来源与内容；无要求时明确「沿用上一轮结论」。
    #[test]
    fn round_intro_states_round_and_input() {
        let with_input = round_intro(2, Some("翅膀握住车把，腿自然弯曲"));
        assert!(with_input.contains("第 2 轮"), "{with_input}");
        assert!(with_input.contains("翅膀握住车把"), "{with_input}");

        let without = round_intro(3, None);
        assert!(without.contains("第 3 轮"), "{without}");
        assert!(without.contains("沿用上一轮结论"), "{without}");

        let blank = round_intro(2, Some("   "));
        assert!(blank.contains("沿用上一轮结论"), "{blank}");
    }

    /// 面向用户的失败原因：常见模型错误翻译成中文+处理建议；未识别保留原文；超长截断。
    #[test]
    fn user_facing_failure_maps_known_errors() {
        let model = user_facing_failure("Model unavailable: provider/DeepSeek V4.1 Flash");
        assert!(model.contains("所选模型不可用"), "{model}");
        assert!(model.contains("换一个模型"), "必须给出处理办法：{model}");

        let funds = user_facing_failure("Upstream request failed: Insufficient account funds");
        assert!(funds.contains("余额不足"), "{funds}");
        assert!(funds.contains("充值"), "必须给出处理办法：{funds}");

        assert_eq!(
            user_facing_failure("Codex 未找到可续聊的目标线程"),
            "Codex 未找到可续聊的目标线程",
            "未识别的错误保留原文（不硬翻译）"
        );

        let long = "长".repeat(FAILURE_DETAIL_LIMIT + 50);
        let truncated = user_facing_failure(&long);
        assert_eq!(truncated.chars().count(), FAILURE_DETAIL_LIMIT + 1);
        assert!(truncated.ends_with('…'));
    }
}
