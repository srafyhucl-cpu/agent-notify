//! OrcTask / 消息构造辅助测试：语境元数据往返、会话语义、NotifyMode。

use a2a_rs_core::{Role, Task, TaskState};
use agentnotify_orchestration::{
    NotifyMode, OrcErrorCode, OrcTask, Workflow, continue_message, new_session_message,
};

/// 新建任务：当前步骤 1、状态 Working、语境字段齐备、context_id = 任务 id（新会话语义）。
#[test]
fn new_task_has_orchestration_context() {
    let wf = Workflow::preset(false).unwrap();
    let task = OrcTask::new(&wf, "做一个贪吃蛇游戏", NotifyMode::Verbose).unwrap();

    assert_eq!(task.state(), TaskState::Working);
    assert_eq!(task.current_step().unwrap(), 1);
    assert_eq!(task.workflow_id().unwrap(), wf.id);
    assert_eq!(task.goal().unwrap(), "做一个贪吃蛇游戏");
    assert_eq!(task.notify_mode().unwrap(), NotifyMode::Verbose);
    assert_eq!(task.blocked_step().unwrap(), None);
    assert_eq!(
        task.a2a_task.context_id,
        task.id(),
        "一个编排任务即一个逻辑会话"
    );
}

/// A2A Task 直出：kind 为内部字段、status 携带 Working（唯一事实源可序列化）。
#[test]
fn new_task_wire_serializes_fact_source() {
    let wf = Workflow::preset(false).unwrap();
    let task = OrcTask::new(&wf, "目标", NotifyMode::FinalOnly).unwrap();
    let json = serde_json::to_string(&task.a2a_task).unwrap();
    assert!(json.contains("\"id\":\""), "{json}");
    assert!(json.contains("TASK_STATE_WORKING"), "{json}");
    // 编排语境随 metadata 落库，可被 from_a2a 还原
    let restored = OrcTask::from_a2a(task.a2a_task).unwrap();
    assert_eq!(restored.current_step().unwrap(), 1);
    assert_eq!(restored.workflow_id().unwrap(), wf.id);
}

/// metadata 缺编排键 / 非对象 → 明确报错（MetaInvalid），不做猜测兜底。
#[test]
fn from_a2a_rejects_missing_or_broken_meta() {
    let wf = Workflow::preset(false).unwrap();
    let good = OrcTask::new(&wf, "目标", NotifyMode::FinalOnly).unwrap();

    // 缺 metadata
    let mut bare = good.a2a_task.clone();
    bare.metadata = None;
    let err = OrcTask::from_a2a(bare).unwrap_err();
    assert_eq!(err.code, OrcErrorCode::MetaInvalid);
    assert!(err.message.contains("metadata"), "{}", err.message);

    // metadata 存在但缺 "orc" 键
    let mut no_key = good.a2a_task.clone();
    no_key.metadata = Some(serde_json::json!({"other": 1}));
    let err = OrcTask::from_a2a(no_key).unwrap_err();
    assert_eq!(err.code, OrcErrorCode::MetaInvalid);

    // metadata 不是 JSON 对象
    let mut not_obj = good.a2a_task.clone();
    not_obj.metadata = Some(serde_json::json!("oops"));
    let err = OrcTask::from_a2a(not_obj).unwrap_err();
    assert_eq!(err.code, OrcErrorCode::MetaInvalid);

    // "orc" 值损坏（缺字段）
    let mut broken = good.a2a_task.clone();
    broken.metadata = Some(serde_json::json!({"orc": {"workflowId": "x"}}));
    let err = OrcTask::from_a2a(broken).unwrap_err();
    assert_eq!(err.code, OrcErrorCode::MetaInvalid);
    assert!(err.message.contains("损坏"), "{}", err.message);
}

/// set_current_step / set_blocked / clear_blocked 持久化到 A2A Task.metadata。
#[test]
fn context_mutations_persist_in_metadata() {
    let wf = Workflow::preset(false).unwrap();
    let mut task = OrcTask::new(&wf, "目标", NotifyMode::FinalOnly).unwrap();

    task.set_current_step(3).unwrap();
    task.set_blocked(2, "commandcode 会话不可用（未登录）")
        .unwrap();

    let json = serde_json::to_string(&task.a2a_task).unwrap();
    assert!(json.contains("\"currentStep\":3"), "{json}");
    assert!(json.contains("\"blockedStep\":2"), "{json}");
    assert!(json.contains("\"blockReason\""), "{json}");

    let restored = OrcTask::from_a2a(task.a2a_task).unwrap();
    assert_eq!(restored.current_step().unwrap(), 3);
    assert_eq!(restored.blocked_step().unwrap(), Some(2));

    let mut cleared = restored;
    cleared.clear_blocked().unwrap();
    assert_eq!(cleared.blocked_step().unwrap(), None);
}

/// 新会话消息：task_id/context_id 均为 None（A2A 语义 = 发起新任务），wire 上不出现。
#[test]
fn new_session_message_has_no_ids() {
    let msg = new_session_message(Role::User, "开始");
    assert!(msg.task_id.is_none());
    assert!(msg.context_id.is_none());
    assert!(!msg.message_id.is_empty(), "消息 id 不能为空");
    assert_eq!(msg.role, Role::User);
    assert_eq!(msg.parts.len(), 1);
    assert_eq!(msg.parts[0].as_text(), Some("开始"));

    let json = serde_json::to_string(&msg).unwrap();
    assert!(!json.contains("taskId"), "{json}");
    assert!(!json.contains("contextId"), "{json}");
}

/// 续聊消息：带 taskId + contextId（wire 名 camelCase）。
#[test]
fn continue_message_has_task_and_context_ids() {
    let msg = continue_message(Role::Agent, "继续干", "task-1", "ctx-1");
    assert_eq!(msg.task_id.as_deref(), Some("task-1"));
    assert_eq!(msg.context_id.as_deref(), Some("ctx-1"));
    assert_eq!(msg.parts[0].as_text(), Some("继续干"));

    let json = serde_json::to_string(&msg).unwrap();
    assert!(json.contains("\"taskId\":\"task-1\""), "{json}");
    assert!(json.contains("\"contextId\":\"ctx-1\""), "{json}");
}

/// NotifyMode 默认 FinalOnly；wire 取值 final_only / verbose（与 TASK.notify_mode 一致）。
#[test]
fn notify_mode_default_and_wire_values() {
    assert_eq!(NotifyMode::default(), NotifyMode::FinalOnly);
    assert_eq!(NotifyMode::FinalOnly.as_str(), "final_only");
    assert_eq!(NotifyMode::Verbose.as_str(), "verbose");

    let v: serde_json::Value = serde_json::to_value(NotifyMode::Verbose).unwrap();
    assert_eq!(v, serde_json::json!("verbose"));
    let v: serde_json::Value = serde_json::to_value(NotifyMode::FinalOnly).unwrap();
    assert_eq!(v, serde_json::json!("final_only"));

    let parsed: NotifyMode = serde_json::from_str("\"verbose\"").unwrap();
    assert_eq!(parsed, NotifyMode::Verbose);
}

/// 纯净 A2A Task（无编排语境）改造后也能正确包装（如外部任务接管场景）。
#[test]
fn wrap_foreign_task_with_meta() {
    let wf = Workflow::preset(false).unwrap();
    let mut task = OrcTask::new(&wf, "接管任务", NotifyMode::FinalOnly).unwrap();
    // 模拟外部把 A2A Task 状态推进到等汇报
    task.set_state(TaskState::InputRequired);
    let restored = OrcTask::from_a2a(task.a2a_task).unwrap();
    assert_eq!(restored.state(), TaskState::InputRequired);
}

/// 状态更新走包装方法后 wire 序列化一致（Working → InputRequired ↔ waiting_report）。
#[test]
fn state_update_reflects_in_a2a_task() {
    let wf = Workflow::preset(false).unwrap();
    let mut task = OrcTask::new(&wf, "目标", NotifyMode::FinalOnly).unwrap();
    task.set_state(TaskState::InputRequired);
    let json = serde_json::to_string(&task.a2a_task).unwrap();
    assert!(json.contains("TASK_STATE_INPUT_REQUIRED"), "{json}");
}

/// 类型别名：OrcTask 可还原成纯 A2A Task 交还协议层（不丢失任何字段）。
#[test]
fn orc_task_degrades_to_a2a_task() {
    let wf = Workflow::preset(false).unwrap();
    let orc = OrcTask::new(&wf, "目标", NotifyMode::Verbose).unwrap();
    let a2a: Task = orc.a2a_task;
    assert_eq!(a2a.status.state, TaskState::Working);
    assert!(a2a.metadata.is_some());
}

/// 工作目录：创建默认空（旧任务兼容），set/clear 往返；旧 meta JSON 缺字段仍可解析。
#[test]
fn working_dir_roundtrip_and_legacy_compat() {
    let wf = Workflow::preset(false).unwrap();
    let mut task = OrcTask::new(&wf, "目标", NotifyMode::FinalOnly).unwrap();
    assert_eq!(task.working_dir().unwrap(), None);

    task.set_working_dir("D:/Project/demo").unwrap();
    assert_eq!(
        task.working_dir().unwrap().as_deref(),
        Some("D:/Project/demo")
    );

    task.set_working_dir("   ").unwrap();
    assert_eq!(task.working_dir().unwrap(), None, "空串清除工作目录");

    // 旧任务 meta（无 workingDir/stepReports/finalReportPending 字段）仍可解析。
    let mut legacy = task.a2a_task.clone();
    let meta = legacy
        .metadata
        .as_mut()
        .and_then(|value| value.get_mut("orc"))
        .and_then(serde_json::Value::as_object_mut)
        .unwrap();
    meta.remove("workingDir");
    meta.remove("stepReports");
    meta.remove("finalReportPending");
    let restored = OrcTask::from_a2a(legacy).unwrap();
    assert_eq!(restored.working_dir().unwrap(), None);
    assert!(restored.step_report(1).unwrap().is_none());
    assert!(!restored.is_finalizing().unwrap());
}

/// 步骤产出：单步超限截断带标注、同一步覆盖、总量超限从最早步骤裁剪。
#[test]
fn step_reports_are_bounded() {
    use agentnotify_orchestration::{ORC_STEP_REPORT_LIMIT, ORC_STEP_REPORTS_TOTAL_LIMIT};

    let wf = Workflow::preset(true).unwrap(); // 4 步
    let mut task = OrcTask::new(&wf, "目标", NotifyMode::FinalOnly).unwrap();

    let long = "字".repeat(ORC_STEP_REPORT_LIMIT + 500);
    task.record_step_report(1, &long).unwrap();
    let body = task.step_report(1).unwrap().unwrap();
    assert!(body.ends_with("…（已截断）"), "超限必须标注截断");
    assert!(
        body.chars().count() <= ORC_STEP_REPORT_LIMIT + 8,
        "单步必须按上限截断"
    );

    task.record_step_report(1, "短产出").unwrap();
    assert_eq!(
        task.step_report(1).unwrap().as_deref(),
        Some("短产出"),
        "同一步覆盖旧值"
    );

    // 总量保护：每步都塞满上限，最终总量不超过总上限（允许标注带来的少量超出）。
    for step in 2..=4 {
        task.record_step_report(step, &long).unwrap();
    }
    let meta = task.meta().unwrap();
    let total: usize = meta
        .step_reports
        .iter()
        .map(|report| report.body.chars().count())
        .sum();
    assert!(
        total <= ORC_STEP_REPORTS_TOTAL_LIMIT + 16,
        "总量必须被裁剪：{total}"
    );
}

/// 汇总阶段标记：默认 false；置位/复位往返。
#[test]
fn final_report_pending_roundtrip() {
    let wf = Workflow::preset(false).unwrap();
    let mut task = OrcTask::new(&wf, "目标", NotifyMode::FinalOnly).unwrap();
    assert!(!task.is_finalizing().unwrap());
    task.set_final_report_pending(true).unwrap();
    assert!(task.is_finalizing().unwrap());
    task.set_final_report_pending(false).unwrap();
    assert!(!task.is_finalizing().unwrap());
}
