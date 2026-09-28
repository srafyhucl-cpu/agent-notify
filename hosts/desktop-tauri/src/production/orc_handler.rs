//! 编排命令处理器：任务创建/开始/推进/汇报回注/派活/呈现（2026-09 从 `service.rs` 拆分）。
//!
//! 与 `service.rs`（桥接命令门面）解耦：本模块只做编排业务；桌面命令经
//! `HostCommandService` 实现转发到 [`OrcCommandHandler`]。

use std::path::Path;
use std::sync::Arc;

use agentnotify_domain::{AgentId, AgentSessionId};
use agentnotify_orchestration::{
    MessageKind, NotifyMode, OrcError, OrcStore, OrcTask, StepOutcome, TaskState, TemplateResolver,
    TransitionAction, Workflow,
};
use agentnotify_storage_sqlite::SqliteStore;

use super::agent_driver::AgentDriver;
use super::orc_node_config::{NodeConfig, template_dtos, validate_template_steps};
use super::orc_notify::{
    OrcClusterPresenter, failure_body, progress_body, render_cluster_message, should_notify,
};
use super::orc_wechat_route::state_cn;
use super::settings::{KEY_ORCHESTRATION_NODE_CONFIG, ProductionSettingsStore};
use crate::bridge::dto::*;
use crate::bridge::error::CommandError;

/// 编排命令处理器（P1-1，B 方案接线）。
///
/// 两种装配模式：
/// - **静态（测试/向后兼容）**：`store: Option<OrcStore>` 启动时一次性定（`new`/`with_templates`
///   /`with_presenter`/`with_driver`），`orchestration.enabled` 由启动装配决定，改设置需重启；
/// - **动态（生产，P1-1 体验修正）**：`with_selector` 注入 `Arc<SqliteStore>` + settings——
///   每次命令时按 `orchestration.enabled`（**默认开启**）与 `orchestration.workflow` 实时解析，
///   **改设置立即生效，无需重启**。
pub struct OrcCommandHandler {
    store: Option<OrcStore>,
    /// 动态模式：命令时按 settings 解析 enabled/workflow 构建 OrcStore（`orchestration_store_for`）。
    sqlite: Option<Arc<SqliteStore>>,
    settings: Option<ProductionSettingsStore>,
    /// harness 模板解析器（P1-5）：用户模板优先、内置默认兜底；派活时由此生成任务信封。
    templates: TemplateResolver,
    /// 集群消息呈现（P1-4）：缺省不呈现（行为与 P1-3 一致）；注入后 advance/mark_blocked 按通知节奏外发。
    presenter: Option<Arc<dyn OrcClusterPresenter>>,
    /// 派活驱动器（P2）：缺省不派活（行为与 P1-3/1-4 一致）；注入后 create/advance
    /// 把当前 Step 的任务信封真正交给配置的 Agent。
    driver: Option<Arc<dyn AgentDriver>>,
}

/// 编排开关设置键（settings 表）。**默认开启**（缺失 = true，P1-1 体验修正：
/// 编排是桌面端核心能力，不应默认关掉让用户困惑）；显式 false 才关闭。
pub const KEY_ORCHESTRATION_ENABLED: &str = "orchestration.enabled";
/// 编排工作流选择设置键（settings 表）：`opencode-only` 只用 OpenCode 单 Agent；其它/缺失 = 默认多 Agent 委托。
pub const KEY_ORCHESTRATION_WORKFLOW: &str = "orchestration.workflow";
/// 全局默认通知节奏设置键（settings 表，P1-4 §4.6）：缺失/非法回退 `final_only` 并告警。
pub const KEY_ORCHESTRATION_NOTIFY_MODE: &str = "orchestration.notify_mode";
/// 用户 harness 模板配置文件（`config_dir` 下，§4.3 / P1-5；缺失 = 内置默认兜底）。
pub const HARNESS_TEMPLATES_FILE: &str = "harness-templates.json";
const ORCHESTRATION_DISABLED_CODE: &str = "orchestration_disabled";
const ORCHESTRATION_DISABLED_MESSAGE: &str =
    "编排未启用：请在设置中启用 orchestration.enabled 后重启应用";
/// 当前步骤未配置 Agent（agent_hint 缺失）时的稳定错误码（P2 派活，写进 blocked 原因）。
const ORC_STEP_AGENT_MISSING: &str = "orc_step_agent_missing";
/// 创建任务的模板不存在（新任务只开放内置三档模板）。
const ORC_TEMPLATE_UNKNOWN: &str = "orc_template_unknown";
/// 工作目录为空或不是已存在目录。
const ORC_WORKING_DIR_INVALID: &str = "orc_working_dir_invalid";
/// 编排设置存储不可用（静态装配/初始化未完成）时保存节点配置的错误码。
const ORC_SETTINGS_UNAVAILABLE: &str = "orchestration_settings_unavailable";
/// 派活信封会话 id 前缀：`task-<task_id>-step-<n>`（每个 (task, step) 一个稳定会话）。
const ORC_DISPATCH_SESSION_PREFIX: &str = "task";

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
        }
    }

    /// 动态装配（生产，P1-1 体验修正）：每次命令时按 settings 实时解析
    /// `orchestration.enabled`（**默认开启**）与 `orchestration.workflow`，改设置立即生效无需重启。
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
        }
    }

    /// 模板解析器访问：派活生成任务信封时用（用户模板优先、内置默认兜底）。
    pub fn templates(&self) -> &TemplateResolver {
        &self.templates
    }

    /// 解析当前可用的 [`OrcStore`]（克隆，代价可忽略；调用方法都 await）。
    ///
    /// - 静态模式（测试/向后兼容）：启动时装配的 `store` 原样返回（改设置需重启，语义不变）；
    /// - 动态模式（生产，P1-1 体验修正）：每次命令时按 settings 实时解析——
    ///   `orchestration.enabled` **默认开启**（缺失/异常按 true，显式 false 才关闭），
    ///   `orchestration.workflow` 决定预置工作流（`opencode-only` 单 Agent / 默认多 Agent 委托）；
    ///   **改设置立即生效，无需重启**。未启用返回名明确错误 `orchestration_disabled`。
    async fn resolve_store(&self) -> Result<OrcStore, CommandError> {
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

    pub async fn create(&self, payload: CreateOrcTaskPayload) -> Result<OrcTaskDto, CommandError> {
        let base_store = self.resolve_store().await?;
        let goal = payload.goal.trim();
        if goal.is_empty() {
            return Err(CommandError::new("orc_goal_empty", "任务目标不能为空"));
        }
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
        let merged = self.node_config().await.merge_workflow(&workflow);
        let store = OrcStore::with_repository(merged, base_store.repository());
        let mut task = store
            .create_task(goal, notify_mode)
            .await
            .map_err(orc_error)?;
        task.set_working_dir(working_dir).map_err(orc_error)?;
        store.save(task.clone()).await.map_err(orc_error)?;
        // 创建后**不自动派活**：任务先进入「待开始」（started=false），用户在界面上
        // 看清工作流节点（每步角色与派给谁）后点「开始执行」再派活第 1 步。
        // 这样避免"没看清节点就被派活"，也避免默认工作流与实际可用 Agent 不匹配时的意外阻塞。
        let dto = orc_task_to_dto(&task, store.workflow())?;
        Ok(dto)
    }

    /// 开始执行（人工确认后）：把「待开始」任务标记为已开始，并派活第 1 步。
    ///
    /// 已开始的任务再调 → 明确错误；未启用/任务不存在 → 与其它命令一致的明确错误。
    pub async fn start(&self, payload: OrcTaskIdPayload) -> Result<OrcTaskDto, CommandError> {
        let store = self.resolve_store().await?;
        let mut task = store.get_task(&payload.task_id).await.map_err(orc_error)?;
        if task.is_started().map_err(orc_error)? {
            return Err(CommandError::new(
                "orc_task_already_started",
                "任务已开始执行，无需重复开始",
            ));
        }
        task.mark_started().map_err(orc_error)?;
        store.save(task.clone()).await.map_err(orc_error)?;
        // 开始即派活第 1 步（新会话开工，open=true）：失败只标记 blocked（§4.6 不自动重推）
        // + 呈现层推失败提醒，不影响已落库的开始结果与命令返回。
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
        let store = self.resolve_store().await?;
        let tasks = store.list_tasks().await.map_err(orc_error)?;
        let workflow = store.workflow();
        tasks
            .iter()
            .map(|task| orc_task_to_dto(task, workflow))
            .collect()
    }

    /// 当前编排工作流（预置选择与节点列表）：供创建任务前预览「每步做什么、派给谁」。
    pub async fn current_workflow(&self) -> Result<CurrentOrcWorkflowDto, CommandError> {
        let store = self.resolve_store().await?;
        Ok(CurrentOrcWorkflowDto {
            workflow: orc_workflow_to_dto(store.workflow()),
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
    async fn node_config(&self) -> NodeConfig {
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
        let mut values = std::collections::BTreeMap::new();
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

    pub async fn advance(
        &self,
        payload: AdvanceOrcTaskPayload,
    ) -> Result<OrcTaskDto, CommandError> {
        let store = self.resolve_store().await?;
        // 未开始的任务不接受推进（先点「开始执行」；也覆盖微信侧对未开始任务的指令）。
        let current = store.get_task(&payload.task_id).await.map_err(orc_error)?;
        if !current.is_started().map_err(orc_error)? {
            return Err(CommandError::new(
                "orc_task_not_started",
                "任务尚未开始执行：请先在集群页点「开始执行」",
            ));
        }
        let kind = parse_message_kind(payload.kind);
        let outcome = store
            .on_message(&payload.task_id, kind)
            .await
            .map_err(orc_error)?;
        let task = store.get_task(&payload.task_id).await.map_err(orc_error)?;
        let dto = orc_task_to_dto(&task, store.workflow())?;
        // P1-4 呈现层单一入口：推进后按任务通知节奏决定是否外发微信（失败不阻塞命令结果）。
        if let Some(presenter) = &self.presenter {
            self.present_advance(presenter, &store, &task, &dto, kind, &outcome)
                .await;
        }
        // P2 派活：Advance/BackToWork/Recover 且任务未完成 → 把当前目标 Step 的信封
        // 交给该步配置的 Agent（失败只标记 blocked，不改变已落库的推进结果）。
        self.dispatch_current_step(&store, &task, &outcome).await;
        Ok(dto)
    }

    /// Agent 汇报自动回注（§4.4「该 Step 的 Agent 汇报到达」）：
    /// 由 [`OrcReportObserver`] 在 session 完成事件匹配 `task-<id>-step-<n>` 时调用——
    /// 校验任务正处于第 n 步且干活中，然后复用 [`Self::advance`]（Report）完成推进
    /// （含通知节奏呈现与下一步派活），保证与人工推进同一条链路。
    ///
    /// 返回 `Ok(false)` = 过期/无关事件（任务不存在、步不一致、任务非干活中），静默忽略；
    /// 错误 = 回注自身失败（由调用方记日志，不影响事件消费）。
    pub async fn report_from_agent(&self, task_id: &str, step: u32) -> Result<bool, CommandError> {
        let store = self.resolve_store().await?;
        let task = match store.get_task(task_id).await {
            Ok(task) => task,
            Err(_) => return Ok(false),
        };
        if task.current_step().ok() != Some(step)
            || task.state() != TaskState::Working
            || !task.is_started().unwrap_or(false)
        {
            return Ok(false);
        }
        self.advance(AdvanceOrcTaskPayload {
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
        store: &OrcStore,
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
        let total = store.workflow().max_order();
        let step = dto.current_step;
        // 正文里的"收到汇报的那一步"：Advance 后 current_step 已指向下一步，需回退一步。
        let reported_step = if outcome.action == TransitionAction::Advance {
            step.saturating_sub(1)
        } else {
            step
        };
        let body = progress_body(kind, outcome.action, reported_step);
        let text = render_cluster_message(task.id(), step, total, state_cn(&dto.state), &body);
        presenter.push(task.id(), text).await;
    }

    pub async fn mark_blocked(
        &self,
        payload: MarkBlockedOrcTaskPayload,
    ) -> Result<OrcTaskDto, CommandError> {
        let store = self.resolve_store().await?;
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
            self.present_blocked(presenter, &store, task.id(), payload.step, reason)
                .await;
        }
        Ok(dto)
    }

    /// P1-4 失败提醒呈现：写清哪一步失败、原因，需人工处理（会话语义与命令 mark_blocked 一致）。
    async fn present_blocked(
        &self,
        presenter: &Arc<dyn OrcClusterPresenter>,
        store: &OrcStore,
        task_id: &str,
        step: u32,
        reason: &str,
    ) {
        let total = store.workflow().max_order();
        let text = render_cluster_message(
            task_id,
            step,
            total,
            state_cn(&OrcTaskStateDto::Failed),
            &failure_body(step, reason),
        );
        presenter.push(task_id, text).await;
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
    ///   （写清「Step N 未配置 Agent」，用户可改工作流后恢复）。
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
        let Some(step) = store.workflow().step(current_step) else {
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
        // 信封渲染（用户模板优先、内置默认兜底，§4.3 / P1-5）：渲染告警只记日志不阻断。
        let next_role = store
            .workflow()
            .next_step(current_step)
            .map(|next| next.role.as_str());
        let rendered = self
            .templates
            .render_envelope(store.workflow(), step, &goal, next_role);
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
        if let Err(error) = driver
            .dispatch(task.id(), &agent_id, &session_id, &rendered.text, open)
            .await
        {
            tracing::warn!(
                task_id = %task.id(),
                step = current_step,
                code = error.code(),
                "派活失败：{}",
                error.message()
            );
            self.auto_blocked(
                store,
                task.id(),
                current_step,
                format!("Step {current_step} 派活失败：{}", error.message()),
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
                    self.present_blocked(presenter, store, task.id(), step, &reason)
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

    pub async fn recover_blocked(
        &self,
        payload: OrcTaskIdPayload,
    ) -> Result<OrcTaskDto, CommandError> {
        let store = self.resolve_store().await?;
        let task = store
            .recover_blocked(&payload.task_id)
            .await
            .map_err(orc_error)?;
        orc_task_to_dto(&task, store.workflow())
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
        working_dir: meta.working_dir,
        finalizing: meta.final_report_pending,
    })
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
