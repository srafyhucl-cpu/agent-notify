//! 任务信封模板解析（P1-5，§4.3 / R10）：用户模板优先、内置默认兜底，告警不静默。
//!
//! 模板来源优先级（从高到低，§4.3「分工界线」）：
//! 1. [`WorkflowStep`] 自带的 `harness_template`（代码/程序化配置，P0 已有行为）；
//! 2. 用户配置文件（按 `workflow_id` + `step order` 匹配，本模块）；
//! 3. 内置默认模板 [`DEFAULT_ENVELOPE_TEMPLATE`] 兜底。
//!
//! 配置格式（JSON，desktop 装配时读 `config_dir/harness-templates.json`）：
//! ```json
//! {
//!   "workflows": {
//!     "preset-requirement-to-report": {
//!       "summary": "【{goal}】汇总：{reports}",
//!       "steps": [
//!         { "order": 1, "harness_template": "【{goal}】…" }
//!       ]
//!     }
//!   }
//! }
//! ```
//! `summary` 为可选的「汇总信封」模板（§4 项目经理回流），缺省用内置默认。
//!
//! 错误语义（核心原则「坏模板不炸」）：
//! - 文件缺失 = 正常未配置，回退内置默认（无告警）；
//! - 整份 JSON 损坏 = 一条告警 + 全部用户模板丢弃；
//! - 单个 workflow 条目损坏（类型错等）= 该条告警 + 该条丢弃，其余生效；
//! - 单个 step 非法（order=0 / 序号重复 / 模板空白）= 该步告警 + 跳过；
//! - 模板含未知占位符 = 告警 + 原样保留（渲染层面与 P0 行为一致，未知占位符不替换）。
//!
//! 告警一律通过 [`TemplateWarning`] / [`TemplateLoadResult`] 明确暴露（不静默），
//! 由装配方记入日志/诊断；解析失败只影响对应工作流/步骤的信封文案，不影响编排运行。

use std::collections::BTreeMap;
use std::path::Path;

use crate::envelope::{
    DEFAULT_SUMMARY_ENVELOPE_TEMPLATE, PH_AGENT_HINT, PH_GOAL, PH_NEXT_ROLE, PH_REPORTS, PH_ROLE,
    PH_ROUND, PH_STEP_INDEX, PH_STEP_TOTAL, PH_WORKFLOW_NAME, default_envelope_template,
    render_summary_with_template, render_with_template,
};
use crate::workflow::{Workflow, WorkflowStep};

/// 全部已知占位符（命名常量聚合，供未知占位符校验，避免魔法字符串散落）。
pub const KNOWN_PLACEHOLDERS: [&str; 9] = [
    PH_GOAL,
    PH_WORKFLOW_NAME,
    PH_ROLE,
    PH_AGENT_HINT,
    PH_STEP_INDEX,
    PH_STEP_TOTAL,
    PH_NEXT_ROLE,
    PH_ROUND,
    PH_REPORTS,
];

/// 模板来源：用于日志/诊断，让用户看到自己的模板是否生效、回退到了哪一层。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TemplateSource {
    /// 步骤自带 `harness_template`（P0 已有行为）
    StepOverride,
    /// 配置文件用户模板（本模块，P1-5）
    UserConfig,
    /// 内置默认模板兜底
    BuiltinDefault,
}

impl TemplateSource {
    /// 稳定字符串（日志可程序化过滤）。
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::StepOverride => "step_override",
            Self::UserConfig => "user_config",
            Self::BuiltinDefault => "builtin_default",
        }
    }
}

/// 模板告警分类（稳定码）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TemplateWarningKind {
    /// 配置文件存在但读取失败（文件不存在不算告警 = 正常未配置）
    ConfigUnreadable,
    /// 整份配置 JSON 解析失败 → 全部用户模板丢弃，回退默认
    ConfigInvalid,
    /// 单个 workflow/step 条目结构错误（类型错/序号非法/重复）→ 该条丢弃
    EntryInvalid,
    /// 模板含未知占位符 → 原样保留，仅告警（坏模板不炸）
    UnknownPlaceholder,
}

impl TemplateWarningKind {
    /// 稳定字符串（日志可程序化过滤）。
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::ConfigUnreadable => "template.config_unreadable",
            Self::ConfigInvalid => "template.config_invalid",
            Self::EntryInvalid => "template.entry_invalid",
            Self::UnknownPlaceholder => "template.unknown_placeholder",
        }
    }
}

/// 模板告警：写清哪里失败、为何回退（面向用户的中文消息，不静默）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TemplateWarning {
    pub kind: TemplateWarningKind,
    pub message: String,
}

impl TemplateWarning {
    pub fn new(kind: TemplateWarningKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }
}

/// 单步模板解析结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedTemplate {
    /// 生效模板文本（用户模板或内置默认）。
    pub template: String,
    /// 模板来源。
    pub source: TemplateSource,
    /// 解析告警：None = 本次解析无问题；Some = 有内容问题但模板照用（未知占位符）。
    pub warning: Option<TemplateWarning>,
}

/// 渲染结果：信封文本 + 模板告警（供编排层记日志/诊断，不静默）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderedEnvelope {
    pub text: String,
    pub warnings: Vec<TemplateWarning>,
}

/// 配置文件加载结果：解析器 + 加载告警。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TemplateLoadResult {
    pub resolver: TemplateResolver,
    pub warnings: Vec<TemplateWarning>,
}

/// 任务信封模板解析器（薄层）：持有用户模板映射，内置默认模板兜底。
///
/// orchestration 默认行为（未注入任何用户模板）与 P0 完全一致：`render_envelope`
/// 纯函数照常工作；desktop 装配时经 [`TemplateResolver::from_config_file`] 注入用户模板。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TemplateResolver {
    /// 用户配置模板：workflow_id → step_order（从 1 起）→ 模板文本。
    user_templates: BTreeMap<String, BTreeMap<u32, String>>,
    /// 用户配置的「汇总信封」模板：workflow_id → 模板文本（§4 项目经理回流）。
    user_summary_templates: BTreeMap<String, String>,
}

/// 配置文件顶层结构。`workflows` 值保留为 raw JSON，以便逐工作流容错解析
/// （单条损坏只丢该条，不让整个配置一起失败）。
#[derive(Debug, serde::Deserialize)]
struct HarnessConfigFile {
    #[serde(default)]
    workflows: BTreeMap<String, serde_json::Value>,
}

/// 单个工作流的模板配置：`steps` = 各步信封；`summary` = 汇总信封（缺省 = 内置默认）。
#[derive(Debug, serde::Deserialize)]
struct HarnessWorkflowConfig {
    steps: Vec<HarnessStepConfig>,
    #[serde(default)]
    summary: Option<String>,
}

/// 单步模板配置：`order` = 步骤序号（1 起），`harness_template` 缺失/空白 = 未配置该步。
#[derive(Debug, serde::Deserialize)]
struct HarnessStepConfig {
    order: u32,
    #[serde(default, rename = "harness_template")]
    harness_template: Option<String>,
}

impl TemplateResolver {
    /// 只含内置默认模板（未配置任何用户模板；orchestration 纯业务默认行为）。
    pub fn new() -> Self {
        Self::default()
    }

    /// 直接注入用户模板映射（装配方已有解析好的配置时使用）。
    pub fn with_user_templates(user_templates: BTreeMap<String, BTreeMap<u32, String>>) -> Self {
        Self {
            user_templates,
            user_summary_templates: BTreeMap::new(),
        }
    }

    /// 直接注入用户模板 + 用户汇总信封（装配/测试）。
    pub fn with_user_templates_and_summary(
        user_templates: BTreeMap<String, BTreeMap<u32, String>>,
        user_summary_templates: BTreeMap<String, String>,
    ) -> Self {
        Self {
            user_templates,
            user_summary_templates,
        }
    }

    /// 从配置文本（JSON）解析：整份损坏 → 一条告警 + 全默认；
    /// 单条（工作流/步骤）损坏 → 该条丢并告警，其余生效。
    pub fn from_config_str(json: &str) -> TemplateLoadResult {
        let root: HarnessConfigFile = match serde_json::from_str(json) {
            Ok(root) => root,
            Err(error) => {
                return TemplateLoadResult {
                    resolver: Self::new(),
                    warnings: vec![TemplateWarning::new(
                        TemplateWarningKind::ConfigInvalid,
                        format!(
                            "任务信封模板配置解析失败：{error}；已回退内置默认模板，用户模板全部不生效"
                        ),
                    )],
                };
            }
        };
        let mut user_templates: BTreeMap<String, BTreeMap<u32, String>> = BTreeMap::new();
        let mut user_summary_templates: BTreeMap<String, String> = BTreeMap::new();
        let mut warnings = Vec::new();
        for (workflow_id, raw) in root.workflows {
            let workflow_config: HarnessWorkflowConfig = match serde_json::from_value(raw) {
                Ok(config) => config,
                Err(error) => {
                    warnings.push(TemplateWarning::new(
                        TemplateWarningKind::EntryInvalid,
                        format!(
                            "工作流 {workflow_id} 的模板配置无法解析（{error}）；该工作流用户模板已忽略，回退内置默认"
                        ),
                    ));
                    continue;
                }
            };
            if let Some(summary) = workflow_config.summary {
                if !summary.trim().is_empty() {
                    user_summary_templates.insert(workflow_id.clone(), summary);
                }
            }
            let mut steps: BTreeMap<u32, String> = BTreeMap::new();
            for step in workflow_config.steps {
                let template = step.harness_template.unwrap_or_default();
                if template.trim().is_empty() {
                    // 该步未配置模板 = 正常，跳过（回退内置默认）
                    continue;
                }
                if step.order == 0 {
                    warnings.push(TemplateWarning::new(
                        TemplateWarningKind::EntryInvalid,
                        format!(
                            "工作流 {workflow_id} 存在 order=0 的步骤：步骤序号必须从 1 开始；该步模板已忽略，回退内置默认"
                        ),
                    ));
                    continue;
                }
                if steps.contains_key(&step.order) {
                    warnings.push(TemplateWarning::new(
                        TemplateWarningKind::EntryInvalid,
                        format!(
                            "工作流 {workflow_id} 第 {} 步的模板重复配置：后写入的模板已忽略，保留先配置的",
                            step.order
                        ),
                    ));
                    continue;
                }
                steps.insert(step.order, template);
            }
            if !steps.is_empty() {
                user_templates.insert(workflow_id, steps);
            }
        }
        TemplateLoadResult {
            resolver: Self::with_user_templates_and_summary(user_templates, user_summary_templates),
            warnings,
        }
    }

    /// 从配置文件读取：文件不存在 = 正常未配置（无告警，回退内置默认）；
    /// 存在但读取失败 / 内容损坏 = 明确告警 + 回退默认（不让用户配坏导致编排瘫痪）。
    pub fn from_config_file(path: impl AsRef<Path>) -> TemplateLoadResult {
        let path = path.as_ref();
        let content = match std::fs::read_to_string(path) {
            Ok(content) => content,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                // 未配置 = 正常，不告警
                return TemplateLoadResult {
                    resolver: Self::new(),
                    warnings: Vec::new(),
                };
            }
            Err(error) => {
                return TemplateLoadResult {
                    resolver: Self::new(),
                    warnings: vec![TemplateWarning::new(
                        TemplateWarningKind::ConfigUnreadable,
                        format!(
                            "读取任务信封模板配置文件 {} 失败：{error}；已回退内置默认模板",
                            path.display()
                        ),
                    )],
                };
            }
        };
        Self::from_config_str(&content)
    }

    /// 按工作流 + 步骤序号取用户配置模板；未配置返回 None。
    pub fn user_template(&self, workflow_id: &str, step_order: u32) -> Option<&str> {
        self.user_templates
            .get(workflow_id)
            .and_then(|steps| steps.get(&step_order))
            .map(String::as_str)
    }

    /// 用户模板映射（只读，装配/诊断用）。
    pub fn user_templates(&self) -> &BTreeMap<String, BTreeMap<u32, String>> {
        &self.user_templates
    }

    /// 按工作流取用户配置的汇总信封模板；未配置返回 None。
    pub fn user_summary_template(&self, workflow_id: &str) -> Option<&str> {
        self.user_summary_templates
            .get(workflow_id)
            .map(String::as_str)
    }

    /// 用户汇总信封映射（只读，装配/诊断用）。
    pub fn user_summary_templates(&self) -> &BTreeMap<String, String> {
        &self.user_summary_templates
    }

    /// 解析某工作流某步的模板（优先级：步骤自带 > 用户配置 > 内置默认，§4.3）。
    ///
    /// `step_harness_template` 传 `WorkflowStep.harness_template`；空/空白按未配置处理。
    /// 未知占位符 → 告警 + 原样保留（渲染层面不替换，坏模板不炸）。
    pub fn resolve(
        &self,
        step_harness_template: Option<&str>,
        workflow_id: &str,
        step_order: u32,
        role: &str,
    ) -> ResolvedTemplate {
        let location = format!("第 {step_order} 步");
        match step_harness_template {
            Some(template) if !template.trim().is_empty() => ResolvedTemplate {
                template: template.to_string(),
                source: TemplateSource::StepOverride,
                warning: unknown_placeholder_warning(template, workflow_id, &location),
            },
            _ => match self.user_template(workflow_id, step_order) {
                Some(template) => ResolvedTemplate {
                    template: template.to_string(),
                    source: TemplateSource::UserConfig,
                    warning: unknown_placeholder_warning(template, workflow_id, &location),
                },
                None => ResolvedTemplate {
                    template: default_envelope_template(role).to_string(),
                    source: TemplateSource::BuiltinDefault,
                    warning: None,
                },
            },
        }
    }

    /// 渲染任务信封：模板解析（用户模板优先、按角色的内置默认兜底）+ 占位符替换。
    ///
    /// 与 [`crate::envelope::render_envelope`] 共用替换实现，模板选择按本解析器优先级；
    /// 返回告警供编排层记日志/诊断（不静默）。
    pub fn render_envelope(
        &self,
        workflow: &Workflow,
        step: &WorkflowStep,
        goal: &str,
        next_role: Option<&str>,
        round: u32,
    ) -> RenderedEnvelope {
        let resolved = self.resolve(
            step.harness_template.as_deref(),
            &workflow.id,
            step.order,
            &step.role,
        );
        let text = render_with_template(workflow, step, goal, next_role, round, &resolved.template);
        RenderedEnvelope {
            text,
            warnings: resolved.warning.into_iter().collect(),
        }
    }

    /// 解析某工作流的「汇总信封」模板（优先级：用户配置 > 内置默认；§4 项目经理回流）。
    /// 未知占位符 → 告警 + 原样保留（坏模板不炸）。
    pub fn resolve_summary(&self, workflow_id: &str) -> ResolvedTemplate {
        match self.user_summary_template(workflow_id) {
            Some(template) => ResolvedTemplate {
                template: template.to_string(),
                source: TemplateSource::UserConfig,
                warning: unknown_placeholder_warning(template, workflow_id, "汇总信封"),
            },
            None => ResolvedTemplate {
                template: DEFAULT_SUMMARY_ENVELOPE_TEMPLATE.to_string(),
                source: TemplateSource::BuiltinDefault,
                warning: None,
            },
        }
    }

    /// 渲染「汇总信封」：带各步产出（`reports` 块），发给首节点会话（项目经理）。
    pub fn render_summary_envelope(
        &self,
        workflow: &Workflow,
        goal: &str,
        reports: &str,
        round: u32,
    ) -> RenderedEnvelope {
        let resolved = self.resolve_summary(&workflow.id);
        let text = render_summary_with_template(workflow, goal, reports, round, &resolved.template);
        RenderedEnvelope {
            text,
            warnings: resolved.warning.into_iter().collect(),
        }
    }
}

/// 扫描模板中的未知 `{...}` 占位符；全部已知 → None，否则返回告警
/// （未知占位符原样保留，不参与替换；仅告警，坏模板不炸）。
/// `location` 用中文写清是哪一段模板（如「第 1 步」「汇总信封」）。
fn unknown_placeholder_warning(
    template: &str,
    workflow_id: &str,
    location: &str,
) -> Option<TemplateWarning> {
    let unknown = unknown_placeholders(template);
    if unknown.is_empty() {
        return None;
    }
    Some(TemplateWarning::new(
        TemplateWarningKind::UnknownPlaceholder,
        format!(
            "工作流 {workflow_id} {location}的模板含未知占位符（{}）：将原样保留，不参与替换",
            unknown.join("、")
        ),
    ))
}

/// 提取模板中所有未知占位符；空/含嵌套花括号/纯空白的 `{...}` 视为普通文本，不告警。
fn unknown_placeholders(template: &str) -> Vec<String> {
    let mut out = Vec::new();
    let bytes = template.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'{' {
            if let Some(relative_close) = bytes[i + 1..].iter().position(|&b| b == b'}') {
                let inner = &template[i + 1..i + 1 + relative_close];
                i += relative_close + 2;
                if inner.is_empty() || inner.contains('{') || inner.trim().is_empty() {
                    continue;
                }
                let candidate = format!("{{{inner}}}");
                if !KNOWN_PLACEHOLDERS.iter().any(|known| *known == candidate) {
                    out.push(candidate);
                }
                continue;
            }
        }
        i += 1;
    }
    out
}
