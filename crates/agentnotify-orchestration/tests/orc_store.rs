//! OrcStore 集成测试：create/get/list、消息驱动推进、human_gate、blocked 流转、错误路径。
//! 全部内存态，不碰网络/文件系统。

use agentnotify_orchestration::{
    MessageKind, NotifyMode, OrcErrorCode, OrcStore, TaskState, TransitionAction, Workflow,
};

/// create → get：状态 Working、第 1 步、语境完整。
#[tokio::test]
async fn create_and_get_task() {
    let store = OrcStore::new(Workflow::preset(false).unwrap());
    let task = store
        .create_task("做一个贪吃蛇游戏", NotifyMode::FinalOnly)
        .await
        .unwrap();
    assert_eq!(task.state(), TaskState::Working);
    assert_eq!(task.current_step().unwrap(), 1);
    assert_eq!(task.goal().unwrap(), "做一个贪吃蛇游戏");
    assert_eq!(task.notify_mode().unwrap(), NotifyMode::FinalOnly);

    let fetched = store.get_task(task.id()).await.unwrap();
    assert_eq!(fetched, task);
}

/// 不存在的任务 → 明确报错（TaskNotFound）。
#[tokio::test]
async fn get_missing_task_errors() {
    let store = OrcStore::new(Workflow::preset(false).unwrap());
    let err = store.get_task("no-such-task").await.unwrap_err();
    assert_eq!(err.code, OrcErrorCode::TaskNotFound);
    assert!(err.message.contains("任务不存在"), "{}", err.message);
}

/// 预置 3 步工作流（无复核）：逐次 Report → Advance → Advance → Complete。
#[tokio::test]
async fn report_advances_through_steps_and_completes() {
    let store = OrcStore::new(Workflow::preset(false).unwrap());
    let id = store
        .create_task("做一个贪吃蛇游戏", NotifyMode::FinalOnly)
        .await
        .unwrap()
        .id()
        .to_string();

    let o1 = store.on_message(&id, MessageKind::Report).await.unwrap();
    assert_eq!(o1.action, TransitionAction::Advance);
    assert_eq!(
        store.get_task(&id).await.unwrap().current_step().unwrap(),
        2
    );

    let o2 = store.on_message(&id, MessageKind::Report).await.unwrap();
    assert_eq!(o2.action, TransitionAction::Advance);
    assert_eq!(
        store.get_task(&id).await.unwrap().current_step().unwrap(),
        3
    );

    let o3 = store.on_message(&id, MessageKind::Report).await.unwrap();
    assert_eq!(o3.action, TransitionAction::Complete);
    let done = store.get_task(&id).await.unwrap();
    assert_eq!(
        done.state(),
        TaskState::Completed,
        "最后一步汇报 → 任务完成"
    );
    assert_eq!(done.current_step().unwrap(), 3);
}

/// human_gate 全流程（带复核的 4 步预置）：汇报→等确认（InputRequired）→确认→完成。
/// 关键断言：waiting_report ↔ `TaskState::InputRequired` 映射（§8.3）。
#[tokio::test]
async fn human_gate_full_flow_maps_to_input_required() {
    let store = OrcStore::new(Workflow::preset(true).unwrap());
    let id = store
        .create_task("做一个贪吃蛇游戏", NotifyMode::FinalOnly)
        .await
        .unwrap()
        .id()
        .to_string();

    // 前三步无门：Report 即推进
    for _ in 0..3 {
        let o = store.on_message(&id, MessageKind::Report).await.unwrap();
        assert_eq!(o.action, TransitionAction::Advance);
    }
    assert_eq!(
        store.get_task(&id).await.unwrap().current_step().unwrap(),
        4
    );

    // 第 4 步（复核，human_gate）：汇报 → waiting_report（InputRequired），不推进
    let o = store.on_message(&id, MessageKind::Report).await.unwrap();
    assert_eq!(o.action, TransitionAction::WaitConfirm);
    let waiting = store.get_task(&id).await.unwrap();
    assert_eq!(
        waiting.state(),
        TaskState::InputRequired,
        "waiting_report ↔ InputRequired"
    );
    assert_eq!(waiting.current_step().unwrap(), 4, "等确认期间不推进步骤");

    // 人确认 → 完成
    let o = store.on_message(&id, MessageKind::Confirm).await.unwrap();
    assert_eq!(o.action, TransitionAction::Complete);
    assert_eq!(
        store.get_task(&id).await.unwrap().state(),
        TaskState::Completed
    );
}

/// 未汇报先确认：状态机明确拒绝（ConfirmBeforeReport），任务状态不变。
#[tokio::test]
async fn confirm_before_report_rejected() {
    let store = OrcStore::new(Workflow::preset(false).unwrap());
    let id = store
        .create_task("目标", NotifyMode::FinalOnly)
        .await
        .unwrap()
        .id()
        .to_string();

    let err = store
        .on_message(&id, MessageKind::Confirm)
        .await
        .unwrap_err();
    assert_eq!(err.code, OrcErrorCode::ConfirmBeforeReport);
    assert_eq!(
        store.get_task(&id).await.unwrap().state(),
        TaskState::Working,
        "拒绝后状态不变"
    );
}

/// Instruction：从等确认退回干活（回到 in_progress），步骤不丢；重新汇报再进等确认。
#[tokio::test]
async fn instruction_sends_back_to_work() {
    let store = OrcStore::new(Workflow::preset(true).unwrap());
    let id = store
        .create_task("目标", NotifyMode::FinalOnly)
        .await
        .unwrap()
        .id()
        .to_string();
    for _ in 0..3 {
        store.on_message(&id, MessageKind::Report).await.unwrap();
    }
    store.on_message(&id, MessageKind::Report).await.unwrap();
    assert_eq!(
        store.get_task(&id).await.unwrap().state(),
        TaskState::InputRequired
    );

    let o = store
        .on_message(&id, MessageKind::Instruction)
        .await
        .unwrap();
    assert_eq!(o.action, TransitionAction::BackToWork);
    let back = store.get_task(&id).await.unwrap();
    assert_eq!(back.state(), TaskState::Working, "回到 in_progress");
    assert_eq!(back.current_step().unwrap(), 4, "仍是第 4 步");

    // 重新汇报 → 再次等确认
    let o = store.on_message(&id, MessageKind::Report).await.unwrap();
    assert_eq!(o.action, TransitionAction::WaitConfirm);
    assert_eq!(
        store.get_task(&id).await.unwrap().state(),
        TaskState::InputRequired
    );
}

/// 等确认期间重复汇报：幂等 Stay，不重复推进。
#[tokio::test]
async fn duplicate_report_while_awaiting_confirm_is_idempotent() {
    let store = OrcStore::new(Workflow::preset(true).unwrap());
    let id = store
        .create_task("目标", NotifyMode::FinalOnly)
        .await
        .unwrap()
        .id()
        .to_string();
    for _ in 0..3 {
        store.on_message(&id, MessageKind::Report).await.unwrap();
    }
    store.on_message(&id, MessageKind::Report).await.unwrap();

    let o = store.on_message(&id, MessageKind::Report).await.unwrap();
    assert_eq!(o.action, TransitionAction::Stay);
    assert_eq!(
        store.get_task(&id).await.unwrap().state(),
        TaskState::InputRequired
    );
}

/// 投递失败 → blocked（§4.6）：状态 Failed、记录失败步骤与原因；blocked 中消息被拒。
#[tokio::test]
async fn mark_blocked_records_failure_and_rejects_messages() {
    let store = OrcStore::new(Workflow::preset(false).unwrap());
    let id = store
        .create_task("目标", NotifyMode::FinalOnly)
        .await
        .unwrap()
        .id()
        .to_string();
    store.on_message(&id, MessageKind::Report).await.unwrap(); // 到第 2 步

    let blocked = store
        .mark_blocked(&id, 2, "opencode 会话不可用（未登录），消息未送达")
        .await
        .unwrap();
    assert_eq!(
        blocked.state(),
        TaskState::Failed,
        "blocked ↔ TaskState::Failed"
    );
    assert_eq!(blocked.blocked_step().unwrap(), Some(2));
    let meta = blocked.meta().unwrap();
    assert_eq!(
        meta.block_reason.as_deref(),
        Some("opencode 会话不可用（未登录），消息未送达")
    );

    // blocked 中 Report/Confirm 一律拒绝（不自动重推，等人处理）
    for kind in [MessageKind::Report, MessageKind::Confirm, MessageKind::Info] {
        let err = store.on_message(&id, kind).await.unwrap_err();
        assert_eq!(
            err.code,
            OrcErrorCode::BlockedAwaitingResume,
            "kind={kind:?}"
        );
    }
}

/// 人工重新发起（blocked + Instruction/Recover）：恢复干活、清除阻塞标记。
#[tokio::test]
async fn recover_blocked_resumes_work() {
    let store = OrcStore::new(Workflow::preset(false).unwrap());
    let id = store
        .create_task("目标", NotifyMode::FinalOnly)
        .await
        .unwrap()
        .id()
        .to_string();
    store.mark_blocked(&id, 1, "codex 未登录").await.unwrap();

    let o = store
        .on_message(&id, MessageKind::Instruction)
        .await
        .unwrap();
    assert_eq!(o.action, TransitionAction::Recover);
    let resumed = store.get_task(&id).await.unwrap();
    assert_eq!(resumed.state(), TaskState::Working);
    assert_eq!(resumed.blocked_step().unwrap(), None, "阻塞标记已清除");
    assert_eq!(resumed.current_step().unwrap(), 1, "回到原步骤重新干活");

    // 恢复后消息正常工作
    let o = store.on_message(&id, MessageKind::Report).await.unwrap();
    assert_eq!(o.action, TransitionAction::Advance);
}

/// 显式 recover_blocked 与消息驱动等价；非阻塞任务调用 → 明确报错。
#[tokio::test]
async fn recover_blocked_method_and_errors() {
    let store = OrcStore::new(Workflow::preset(false).unwrap());
    let id = store
        .create_task("目标", NotifyMode::FinalOnly)
        .await
        .unwrap()
        .id()
        .to_string();

    // 未阻塞直接恢复 → TaskNotBlocked
    let err = store.recover_blocked(&id).await.unwrap_err();
    assert_eq!(err.code, OrcErrorCode::TaskNotBlocked);
    assert!(err.message.contains("未处于阻塞状态"), "{}", err.message);

    // 显式恢复路径
    store.mark_blocked(&id, 1, "临时故障").await.unwrap();
    let resumed = store.recover_blocked(&id).await.unwrap();
    assert_eq!(resumed.state(), TaskState::Working);
    assert_eq!(resumed.blocked_step().unwrap(), None);
}

/// 已终止任务不允许标记阻塞（CannotBlockTerminal）。
#[tokio::test]
async fn cannot_block_completed_task() {
    let store = OrcStore::new(Workflow::preset(false).unwrap());
    let id = store
        .create_task("目标", NotifyMode::FinalOnly)
        .await
        .unwrap()
        .id()
        .to_string();
    store.on_message(&id, MessageKind::Report).await.unwrap();
    store.on_message(&id, MessageKind::Report).await.unwrap();
    store.on_message(&id, MessageKind::Report).await.unwrap(); // 完成

    let err = store.mark_blocked(&id, 3, "不该出现").await.unwrap_err();
    assert_eq!(err.code, OrcErrorCode::CannotBlockTerminal);
}

/// 已完成任务不再接受消息（InvalidTaskState，不做猜测兜底）。
#[tokio::test]
async fn completed_task_rejects_messages() {
    let store = OrcStore::new(Workflow::preset(false).unwrap());
    let id = store
        .create_task("目标", NotifyMode::FinalOnly)
        .await
        .unwrap()
        .id()
        .to_string();
    for _ in 0..3 {
        store.on_message(&id, MessageKind::Report).await.unwrap();
    }
    let err = store
        .on_message(&id, MessageKind::Report)
        .await
        .unwrap_err();
    assert_eq!(err.code, OrcErrorCode::InvalidTaskState);
    assert!(err.message.contains("无法映射"), "{}", err.message);
}

/// list_tasks：薄封装返回全部任务（含语境还原）。
#[tokio::test]
async fn list_tasks_returns_all() {
    let store = OrcStore::new(Workflow::preset(false).unwrap());
    store
        .create_task("目标 A", NotifyMode::FinalOnly)
        .await
        .unwrap();
    store
        .create_task("目标 B", NotifyMode::Verbose)
        .await
        .unwrap();
    let tasks = store.list_tasks().await.unwrap();
    assert_eq!(tasks.len(), 2);
    let modes: Vec<_> = tasks.iter().map(|t| t.notify_mode().unwrap()).collect();
    assert!(modes.contains(&NotifyMode::FinalOnly));
    assert!(modes.contains(&NotifyMode::Verbose));
}

/// 两个 store 实例互不干扰（每工作流一个实例的内存仓储）。
#[tokio::test]
async fn store_instances_are_isolated() {
    let store_a = OrcStore::new(Workflow::preset(false).unwrap());
    let store_b = OrcStore::new(Workflow::preset(true).unwrap());
    store_a
        .create_task("A", NotifyMode::FinalOnly)
        .await
        .unwrap();
    store_b.create_task("B", NotifyMode::Verbose).await.unwrap();
    assert_eq!(store_a.list_tasks().await.unwrap().len(), 1);
    assert_eq!(store_b.list_tasks().await.unwrap().len(), 1);
    assert_eq!(
        store_b
            .create_task("B2", NotifyMode::FinalOnly)
            .await
            .unwrap()
            .workflow_id()
            .unwrap(),
        store_b.workflow().id
    );
}
