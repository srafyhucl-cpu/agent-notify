//! 编排节点配置（§3）：settings `orchestration.node_config` 的读写、校验与模板合并。
//!
//! 存储格式（JSON，键为模板 id + 步骤序号字符串）：
//! ```json
//! { "template-standard": { "1": { "agent": "opencode", "model": "anthropic/claude-sonnet-4-5" } } }
//! ```
//! 语义：键缺省 = 该节点未配置；合并时用户配置覆盖内置模板的 `agent_hint`/`model`
//! （空值 = 清除该节点覆盖）。读取/解析失败由调用方告警并按未配置处理（不猜测）。

use std::collections::BTreeMap;

use agentnotify_orchestration::{StepConfigSnapshot, TEMPLATE_IDS, Workflow};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::bridge::dto::{OrcTemplateDto, OrcTemplateStepConfigDto, OrcTemplateStepDto};
use crate::bridge::error::CommandError;

/// 提交的步骤数与 order 与当前模板定义不一致（请求过期：刷新后重试）。
pub const ORC_TEMPLATE_STEPS_INVALID: &str = "orc_template_steps_invalid";
/// 模型格式非法（必须是 `provider/model`，两段都非空）。
pub const ORC_MODEL_INVALID: &str = "orc_model_invalid";
/// 指定了模型但该 Agent 不支持（v1 仅 OpenCode 支持指定模型）。
pub const ORC_MODEL_AGENT_UNSUPPORTED: &str = "orc_model_agent_unsupported";
/// v1 支持「指定模型」的 Agent id。
pub const MODEL_AGENT_OPENCODE: &str = "opencode";

/// 单个节点的用户配置：两值均可空（空值 = 清除该节点覆盖）。
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
pub struct NodeConfigEntry {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
}

impl NodeConfigEntry {
    /// 规范化：去空白后两值都为空 → None（该节点无覆盖，不落盘）。
    pub fn normalize(&self) -> Option<Self> {
        let agent = trimmed_nonempty(self.agent.as_deref());
        let model = trimmed_nonempty(self.model.as_deref());
        if agent.is_none() && model.is_none() {
            return None;
        }
        Some(Self { agent, model })
    }
}

/// 全部节点配置：模板 id → 步骤序号 → 节点覆盖。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct NodeConfig {
    templates: BTreeMap<String, BTreeMap<u32, NodeConfigEntry>>,
}

impl NodeConfig {
    /// 从 settings JSON 解析：整份结构损坏 → 明确原因（调用方告警并按未配置处理）。
    pub fn from_json(value: &Value) -> Result<Self, String> {
        let templates: BTreeMap<String, BTreeMap<u32, NodeConfigEntry>> =
            serde_json::from_value(value.clone())
                .map_err(|error| format!("节点配置 JSON 无法解析（{error}）"))?;
        Ok(Self { templates })
    }

    /// 序列化为 settings JSON：空值节点不落盘；整模板无覆盖时不留空对象。
    pub fn to_json(&self) -> Value {
        let mut templates = serde_json::Map::new();
        for (template_id, steps) in &self.templates {
            let mut step_map = serde_json::Map::new();
            for (order, entry) in steps {
                let Some(entry) = entry.normalize() else {
                    continue;
                };
                let mut object = serde_json::Map::new();
                if let Some(agent) = entry.agent {
                    object.insert("agent".to_string(), Value::String(agent));
                }
                if let Some(model) = entry.model {
                    object.insert("model".to_string(), Value::String(model));
                }
                step_map.insert(order.to_string(), Value::Object(object));
            }
            if !step_map.is_empty() {
                templates.insert(template_id.clone(), Value::Object(step_map));
            }
        }
        Value::Object(templates)
    }

    /// 某模板某步的配置；未配置返回 None。
    pub fn entry(&self, template_id: &str, order: u32) -> Option<&NodeConfigEntry> {
        self.templates
            .get(template_id)
            .and_then(|steps| steps.get(&order))
    }

    /// 覆盖某模板的全部节点配置（空 = 清除该模板配置）。
    pub fn set_template(&mut self, template_id: &str, entries: BTreeMap<u32, NodeConfigEntry>) {
        if entries.is_empty() {
            self.templates.remove(template_id);
        } else {
            self.templates.insert(template_id.to_string(), entries);
        }
    }

    /// 合并模板与用户配置（§3）：用户配置优先；条目存在但字段为空 = 清除该步覆盖。
    pub fn merge_workflow(&self, workflow: &Workflow) -> Workflow {
        let steps = workflow
            .steps
            .iter()
            .map(|step| {
                let Some(entry) = self.entry(&workflow.id, step.order) else {
                    return step.clone();
                };
                let mut merged = step.clone();
                merged.agent_hint = trimmed_nonempty(entry.agent.as_deref());
                merged.model = trimmed_nonempty(entry.model.as_deref());
                merged
            })
            .collect();
        // 步骤来自经校验的工作流，直接复用（顺序/序号不变）。
        Workflow {
            id: workflow.id.clone(),
            name: workflow.name.clone(),
            steps,
        }
    }
}

/// 内置模板列表视图（合并用户配置后）：设置页节点配置与创建任务预览共用。
pub fn template_dtos(node_config: &NodeConfig) -> Vec<OrcTemplateDto> {
    TEMPLATE_IDS
        .iter()
        .filter_map(|id| Workflow::builtin(id))
        .map(|workflow| {
            let merged = node_config.merge_workflow(&workflow);
            OrcTemplateDto {
                id: merged.id,
                name: merged.name,
                steps: merged
                    .steps
                    .iter()
                    .map(|step| OrcTemplateStepDto {
                        order: step.order,
                        role: step.role.clone(),
                        agent: step.agent_hint.clone(),
                        model: step.model.clone(),
                    })
                    .collect(),
            }
        })
        .collect()
}

/// 校验并构建某模板的节点配置（覆盖式全量提交，§3）：
/// - 步骤数与 order 必须与模板一致（否则 `orc_template_steps_invalid`）；
/// - agent 与 model 去空白后均为空 = 该节点无覆盖（清除）；
/// - model 非空必须是 `provider/model` 格式（否则 `orc_model_invalid`）；
/// - model 非空时 agent 必须是 opencode（v1 仅 OpenCode 支持指定模型）→ `orc_model_agent_unsupported`。
pub fn validate_template_steps(
    workflow: &Workflow,
    steps: &[OrcTemplateStepConfigDto],
) -> Result<BTreeMap<u32, NodeConfigEntry>, CommandError> {
    if steps.len() != workflow.steps.len() {
        return Err(CommandError::new(
            ORC_TEMPLATE_STEPS_INVALID,
            format!(
                "模板 {} 共 {} 个节点，提交了 {} 个：请刷新后重试",
                workflow.id,
                workflow.steps.len(),
                steps.len()
            ),
        ));
    }
    let mut entries = BTreeMap::new();
    for (index, step) in steps.iter().enumerate() {
        let expected = workflow.steps[index].order;
        if step.order != expected {
            return Err(CommandError::new(
                ORC_TEMPLATE_STEPS_INVALID,
                format!(
                    "模板 {} 第 {} 个节点序号应为 {}，实际为 {}：请刷新后重试",
                    workflow.id,
                    index + 1,
                    expected,
                    step.order
                ),
            ));
        }
        let agent = trimmed_nonempty(step.agent.as_deref());
        let model = trimmed_nonempty(step.model.as_deref());
        if agent.is_none() && model.is_none() {
            continue;
        }
        if let Some(model) = &model {
            if !is_provider_model(model) {
                return Err(CommandError::new(
                    ORC_MODEL_INVALID,
                    format!("模型格式应为 provider/model：{model}"),
                ));
            }
            if agent.as_deref() != Some(MODEL_AGENT_OPENCODE) {
                return Err(CommandError::new(
                    ORC_MODEL_AGENT_UNSUPPORTED,
                    "该 Agent 暂不支持指定模型",
                ));
            }
        }
        entries.insert(step.order, NodeConfigEntry { agent, model });
    }
    Ok(entries)
}

/// 合并后仍缺 Agent 的第一步（`start` 预检用）；全部已配置返回 None。
pub fn missing_agent_step(workflow: &Workflow) -> Option<u32> {
    workflow
        .steps
        .iter()
        .find(|step| trimmed_nonempty(step.agent_hint.as_deref()).is_none())
        .map(|step| step.order)
}

/// 任务开始时的步骤配置快照（§3）：`agent` 必为预检后的非空 Agent；空 model = 默认模型。
pub fn steps_snapshot_of(workflow: &Workflow) -> Vec<StepConfigSnapshot> {
    workflow
        .steps
        .iter()
        .map(|step| StepConfigSnapshot {
            order: step.order,
            role: step.role.clone(),
            agent: trimmed_nonempty(step.agent_hint.as_deref()).unwrap_or_default(),
            model: trimmed_nonempty(step.model.as_deref()),
        })
        .collect()
}

/// 用任务快照覆盖工作流步骤（按 order 匹配；快照缺该步时保留模板值）：
/// `start` 之后改 settings 节点配置不影响该任务（§3）。
pub fn apply_steps_snapshot(workflow: &Workflow, snapshot: &[StepConfigSnapshot]) -> Workflow {
    let steps = workflow
        .steps
        .iter()
        .map(|step| {
            let Some(entry) = snapshot.iter().find(|entry| entry.order == step.order) else {
                return step.clone();
            };
            let mut merged = step.clone();
            if !entry.role.trim().is_empty() {
                merged.role = entry.role.clone();
            }
            merged.agent_hint = trimmed_nonempty(Some(entry.agent.as_str()));
            merged.model = trimmed_nonempty(entry.model.as_deref());
            merged
        })
        .collect();
    // 步骤来自经校验的工作流，直接复用（顺序/序号不变）。
    Workflow {
        id: workflow.id.clone(),
        name: workflow.name.clone(),
        steps,
    }
}

/// `provider/model` 校验：第一个 `/` 前后都必须非空（模型 id 本身可含 `/`）。
pub fn is_provider_model(value: &str) -> bool {
    value
        .split_once('/')
        .is_some_and(|(provider, id)| !provider.trim().is_empty() && !id.trim().is_empty())
}

/// 去空白；空串按未设置（None）。
fn trimmed_nonempty(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

#[cfg(test)]
mod tests {
    use agentnotify_orchestration::{Workflow, WorkflowStep};

    use super::*;

    fn standard() -> Workflow {
        Workflow::builtin("template-standard").expect("内置模板必须可解析")
    }

    fn standard_quickfix() -> Workflow {
        Workflow::builtin("template-quickfix").expect("内置模板必须可解析")
    }

    /// 合并：用户配置覆盖 agent_hint/model；未配置保留模板值；空值清除。
    #[test]
    fn merge_overrides_template_steps() {
        let mut node_config = NodeConfig::default();
        node_config.set_template(
            "template-standard",
            BTreeMap::from([(
                2,
                NodeConfigEntry {
                    agent: Some(" opencode ".to_string()),
                    model: Some("anthropic/claude-sonnet-4-5".to_string()),
                },
            )]),
        );
        let merged = node_config.merge_workflow(&standard());
        assert_eq!(merged.step(1).unwrap().agent_hint, None, "未配置的步骤不变");
        assert_eq!(
            merged.step(2).unwrap().agent_hint.as_deref(),
            Some("opencode"),
            "用户配置必须去空白后覆盖"
        );
        assert_eq!(
            merged.step(2).unwrap().model.as_deref(),
            Some("anthropic/claude-sonnet-4-5")
        );
        assert_eq!(merged.step(3).unwrap().model, None);

        // 空值条目 = 清除该步覆盖（回到模板原值，本例模板无 hint）。
        node_config.set_template(
            "template-standard",
            BTreeMap::from([(
                2,
                NodeConfigEntry {
                    agent: None,
                    model: None,
                },
            )]),
        );
        let cleared = node_config.merge_workflow(&standard());
        assert_eq!(cleared.step(2).unwrap().agent_hint, None);
    }

    /// 合并对带内置 hint 的旧预设同样生效（用户配置优先）。
    #[test]
    fn merge_overrides_builtin_hints() {
        let preset = Workflow::preset(false).expect("预置工作流必须有效");
        let mut node_config = NodeConfig::default();
        node_config.set_template(
            "preset-requirement-to-report",
            BTreeMap::from([(
                1,
                NodeConfigEntry {
                    agent: Some("opencode".to_string()),
                    model: None,
                },
            )]),
        );
        let merged = node_config.merge_workflow(&preset);
        assert_eq!(
            merged.step(1).unwrap().agent_hint.as_deref(),
            Some("opencode")
        );
        assert_eq!(
            merged.step(2).unwrap().agent_hint.as_deref(),
            Some("opencode"),
            "未覆盖的步骤保持模板原值"
        );
    }

    /// JSON 往返：空值不落盘、空模板不落空对象；解析失败给出中文原因。
    #[test]
    fn json_roundtrip_and_failures() {
        let mut node_config = NodeConfig::default();
        node_config.set_template(
            "template-quickfix",
            BTreeMap::from([
                (
                    1,
                    NodeConfigEntry {
                        agent: Some("opencode".to_string()),
                        model: None,
                    },
                ),
                (2, NodeConfigEntry::default()),
            ]),
        );
        let json = node_config.to_json();
        assert_eq!(
            json,
            serde_json::json!({ "template-quickfix": { "1": { "agent": "opencode" } } })
        );

        let parsed = NodeConfig::from_json(&json).expect("往返必须可解析");
        assert_eq!(
            parsed
                .entry("template-quickfix", 1)
                .unwrap()
                .agent
                .as_deref(),
            Some("opencode")
        );

        let error = NodeConfig::from_json(&serde_json::json!("oops")).unwrap_err();
        assert!(error.contains("无法解析"), "{error}");
    }

    /// 保存校验：步骤数/order 一致；模型格式；模型仅 OpenCode；空值 = 清除。
    #[test]
    fn validate_steps_enforces_rules() {
        let workflow = standard(); // 3 步
        let ok = validate_template_steps(
            &workflow,
            &[
                OrcTemplateStepConfigDto {
                    order: 1,
                    agent: Some("opencode".into()),
                    model: Some("anthropic/claude-sonnet-4-5".into()),
                },
                OrcTemplateStepConfigDto {
                    order: 2,
                    agent: Some("codex".into()),
                    model: None,
                },
                OrcTemplateStepConfigDto {
                    order: 3,
                    agent: None,
                    model: None,
                },
            ],
        )
        .expect("合法配置必须通过");
        assert_eq!(ok.len(), 2, "空值节点不落配置");
        assert!(!ok.contains_key(&3));

        let wrong_count = validate_template_steps(
            &workflow,
            &[OrcTemplateStepConfigDto {
                order: 1,
                agent: Some("opencode".into()),
                model: None,
            }],
        )
        .expect_err("步骤数不符必须报错");
        assert_eq!(wrong_count.code(), ORC_TEMPLATE_STEPS_INVALID);

        let wrong_order = validate_template_steps(
            &workflow,
            &[
                OrcTemplateStepConfigDto {
                    order: 1,
                    agent: None,
                    model: None,
                },
                OrcTemplateStepConfigDto {
                    order: 3,
                    agent: None,
                    model: None,
                },
                OrcTemplateStepConfigDto {
                    order: 2,
                    agent: None,
                    model: None,
                },
            ],
        )
        .expect_err("order 不符必须报错");
        assert_eq!(wrong_order.code(), ORC_TEMPLATE_STEPS_INVALID);

        let bad_model = validate_template_steps(
            &workflow,
            &[
                OrcTemplateStepConfigDto {
                    order: 1,
                    agent: Some("opencode".into()),
                    model: Some("no-slash".into()),
                },
                OrcTemplateStepConfigDto {
                    order: 2,
                    agent: None,
                    model: None,
                },
                OrcTemplateStepConfigDto {
                    order: 3,
                    agent: None,
                    model: None,
                },
            ],
        )
        .expect_err("模型格式非法必须报错");
        assert_eq!(bad_model.code(), ORC_MODEL_INVALID);
        assert!(bad_model.message().contains("provider/model"));

        let unsupported_agent = validate_template_steps(
            &workflow,
            &[
                OrcTemplateStepConfigDto {
                    order: 1,
                    agent: Some("codex".into()),
                    model: Some("anthropic/claude-sonnet-4-5".into()),
                },
                OrcTemplateStepConfigDto {
                    order: 2,
                    agent: None,
                    model: None,
                },
                OrcTemplateStepConfigDto {
                    order: 3,
                    agent: None,
                    model: None,
                },
            ],
        )
        .expect_err("非 OpenCode 指定模型必须报错");
        assert_eq!(unsupported_agent.code(), ORC_MODEL_AGENT_UNSUPPORTED);
        assert_eq!(unsupported_agent.message(), "该 Agent 暂不支持指定模型");

        let model_without_agent = validate_template_steps(
            &workflow,
            &[
                OrcTemplateStepConfigDto {
                    order: 1,
                    agent: None,
                    model: Some("anthropic/claude-sonnet-4-5".into()),
                },
                OrcTemplateStepConfigDto {
                    order: 2,
                    agent: None,
                    model: None,
                },
                OrcTemplateStepConfigDto {
                    order: 3,
                    agent: None,
                    model: None,
                },
            ],
        )
        .expect_err("模型非空时 Agent 也必须明确");
        assert_eq!(model_without_agent.code(), ORC_MODEL_AGENT_UNSUPPORTED);
    }

    /// 模型格式：第一个 `/` 前后非空即可；前后空白不算缺失。
    #[test]
    fn provider_model_format() {
        assert!(is_provider_model("anthropic/claude-sonnet-4-5"));
        assert!(is_provider_model("openrouter/anthropic/claude-3.5"));
        assert!(!is_provider_model("anthropic"));
        assert!(!is_provider_model("/claude"));
        assert!(!is_provider_model("anthropic/"));
        assert!(!is_provider_model("   /   "));
    }

    /// 模板列表：三档内置模板按固定顺序返回，并带合并后的节点。
    #[test]
    fn template_dtos_list_builtin_templates() {
        let mut node_config = NodeConfig::default();
        node_config.set_template(
            "template-quickfix",
            BTreeMap::from([(
                1,
                NodeConfigEntry {
                    agent: Some("opencode".into()),
                    model: Some("anthropic/claude-sonnet-4-5".into()),
                },
            )]),
        );
        let templates = template_dtos(&node_config);
        assert_eq!(templates.len(), TEMPLATE_IDS.len());
        assert_eq!(templates[0].id, "template-quickfix");
        assert_eq!(templates[0].steps.len(), 2);
        assert_eq!(templates[0].steps[0].agent.as_deref(), Some("opencode"));
        assert_eq!(
            templates[0].steps[0].model.as_deref(),
            Some("anthropic/claude-sonnet-4-5")
        );
        assert_eq!(templates[1].id, "template-standard");
        assert_eq!(templates[2].id, "template-full");
    }

    /// 预检：合并后缺 Agent 的第一步；全部配置返回 None。
    #[test]
    fn missing_agent_step_reports_first_unconfigured() {
        let workflow = Workflow::new(
            "custom",
            "自定义",
            vec![
                WorkflowStep::new(1, "planner", Some("opencode".to_string()), None, false),
                WorkflowStep::new(2, "executor", None, None, false),
                WorkflowStep::new(3, "reviewer", None, None, false),
            ],
        )
        .expect("自定义工作流必须有效");
        assert_eq!(missing_agent_step(&workflow), Some(2));
        assert_eq!(missing_agent_step(&standard()), Some(1));
        assert_eq!(
            missing_agent_step(&Workflow::preset(false).unwrap()),
            None,
            "预置工作流各步都有 hint"
        );
    }

    /// 快照：从合并后工作流生成（agent 非空、model 可选），并可覆盖模板步骤。
    #[test]
    fn steps_snapshot_locks_effective_config() {
        let mut node_config = NodeConfig::default();
        node_config.set_template(
            "template-quickfix",
            BTreeMap::from([(
                2,
                NodeConfigEntry {
                    agent: Some("opencode".into()),
                    model: Some("anthropic/claude-sonnet-4-5".into()),
                },
            )]),
        );
        let merged = node_config.merge_workflow(&standard_quickfix());
        let snapshot = steps_snapshot_of(&merged);
        assert_eq!(snapshot.len(), 2);
        assert_eq!(snapshot[0].order, 1);
        assert_eq!(
            snapshot[0].agent, "",
            "未配置节点在预检前为空（start 预检会拦下）"
        );
        assert_eq!(snapshot[1].agent, "opencode");
        assert_eq!(
            snapshot[1].model.as_deref(),
            Some("anthropic/claude-sonnet-4-5")
        );

        // 快照覆盖模板（模拟 start 后清空节点配置仍按快照运行）。
        let reapplied = apply_steps_snapshot(&standard_quickfix(), &snapshot);
        assert_eq!(
            reapplied.step(2).unwrap().agent_hint.as_deref(),
            Some("opencode")
        );
        assert_eq!(
            reapplied.step(2).unwrap().model.as_deref(),
            Some("anthropic/claude-sonnet-4-5")
        );

        // 快照缺某步时保留模板值。
        let partial = apply_steps_snapshot(
            &standard_quickfix(),
            &[StepConfigSnapshot {
                order: 1,
                role: "executor".into(),
                agent: "codex".into(),
                model: None,
            }],
        );
        assert_eq!(
            partial.step(1).unwrap().agent_hint.as_deref(),
            Some("codex")
        );
        assert_eq!(partial.step(2).unwrap().agent_hint, None, "缺步保留模板值");
    }
}
