import { describe, expect, it, vi } from "vitest";

import { createMockHostBridge } from "./mockHostBridge";
import { orcTaskFixture } from "../test/fixtures";

describe("HostBridge", () => {
  it("records invocations and returns stable snapshot data", async () => {
    const bridge = createMockHostBridge();
    const first = bridge.invoke("get_snapshot", {});
    const second = bridge.invoke("get_snapshot", {});

    await expect(first).resolves.toMatchObject({
      runtime: { state: "Running", paused: false },
    });
    await expect(second).resolves.toMatchObject({
      runtime: { state: "Running", paused: false },
    });
    expect(bridge.calls("get_snapshot")).toHaveLength(2);
  });

  it("notifies subscribers and removes the listener on unsubscribe", () => {
    const bridge = createMockHostBridge();
    const handler = vi.fn();
    const unsubscribe = bridge.subscribe("snapshot.changed", handler);

    bridge.emit("snapshot.changed", { reason: "runtime.refresh" });
    expect(handler).toHaveBeenCalledOnce();
    expect(handler).toHaveBeenCalledWith({ reason: "runtime.refresh" });

    unsubscribe();
    bridge.emit("snapshot.changed", { reason: "runtime.refresh" });
    expect(handler).toHaveBeenCalledOnce();
  });

  it("allows tests to inject a typed command error", async () => {
    const bridge = createMockHostBridge({
      errors: {
        get_snapshot: {
          code: "runtime_unavailable",
          message: "运行时尚未启动，请稍后重试",
          retryable: true,
        },
      },
    });

    await expect(bridge.invoke("get_snapshot", {})).rejects.toMatchObject({
      code: "runtime_unavailable",
      message: "运行时尚未启动，请稍后重试",
      retryable: true,
    });
  });

  it("allows tests to inject command latency", async () => {
    vi.useFakeTimers();
    const bridge = createMockHostBridge({
      delays: { get_snapshot: 50 },
    });

    const result = bridge.invoke("get_snapshot", {});
    let settled = false;
    void result.then(() => {
      settled = true;
    });

    await vi.advanceTimersByTimeAsync(49);
    expect(settled).toBe(false);

    await vi.advanceTimersByTimeAsync(1);
    await expect(result).resolves.toMatchObject({ runtime: { state: "Running" } });
    vi.useRealTimers();
  });
});

describe("HostBridge 编排命令（P1 集群页）", () => {
  it("list 返回种子任务，create 追加新任务且后继 list 可见", async () => {
    const seeded = orcTaskFixture("task-seed", { goal: "种子任务" });
    const bridge = createMockHostBridge({ orcTasks: [seeded] });

    await expect(bridge.invoke("list_orc_tasks", {})).resolves.toEqual([seeded]);

    const created = await bridge.invoke("create_orc_task", {
      goal: "新任务",
      templateId: "template-standard",
      workingDir: "D:/Project/agent-notify",
      notifyMode: "verbose",
    });
    expect(created).toMatchObject({
      id: "orc-2",
      state: "working",
      currentStep: 1,
      notifyMode: "verbose",
      goal: "新任务",
      workflowId: "template-standard",
      workingDir: "D:/Project/agent-notify",
      finalizing: false,
    });

    await expect(bridge.invoke("list_orc_tasks", {})).resolves.toHaveLength(2);
  });

  it("create 未传 notify_mode 时回落默认 final_only", async () => {
    const bridge = createMockHostBridge();
    const created = await bridge.invoke("create_orc_task", {
      goal: "默认节奏",
      templateId: "template-quickfix",
      workingDir: "D:/Project/agent-notify",
      notifyMode: null,
    });
    expect(created.notifyMode).toBe("final_only");
  });

  it("create 校验模板与工作目录，错误码与后端一致", async () => {
    const bridge = createMockHostBridge();

    await expect(
      bridge.invoke("create_orc_task", {
        goal: "未知模板",
        templateId: "template-missing",
        workingDir: "D:/Project/agent-notify",
        notifyMode: null,
      }),
    ).rejects.toMatchObject({ code: "orc_template_unknown" });

    await expect(
      bridge.invoke("create_orc_task", {
        goal: "空目录",
        templateId: "template-standard",
        workingDir: "   ",
        notifyMode: null,
      }),
    ).rejects.toMatchObject({
      code: "orc_working_dir_invalid",
      message: "工作目录不能为空：请选择 OpenCode 项目或手动输入目录",
    });

    await expect(
      bridge.invoke("create_orc_task", {
        goal: "非法目录",
        templateId: "template-standard",
        workingDir: "not-a-directory",
        notifyMode: null,
      }),
    ).rejects.toMatchObject({
      code: "orc_working_dir_invalid",
      message: "工作目录不存在：not-a-directory",
    });
  });

  it("start 预检：节点缺 Agent 时明确报错且任务保持待开始", async () => {
    const task = orcTaskFixture("task-unconfigured", {
      started: false,
      currentStep: 1,
      workflow: {
        id: "template-standard",
        name: "标准交付",
        steps: [
          { order: 1, role: "planner", agentHint: null, model: null, humanGate: false },
          { order: 2, role: "executor", agentHint: "opencode", model: null, humanGate: false },
        ],
      },
    });
    const bridge = createMockHostBridge({ orcTasks: [task] });

    await expect(
      bridge.invoke("start_orc_task", { taskId: "task-unconfigured" }),
    ).rejects.toMatchObject({
      code: "orc_step_agent_missing",
      message: "第 1 步未选择 Agent：请先在设置 → 编排中配置",
    });

    await expect(bridge.invoke("list_orc_tasks", {})).resolves.toMatchObject([
      { id: "task-unconfigured", started: false },
    ]);
  });

  it("advance 写回推进 currentStep；最后一步完成后转入汇总阶段（finalizing）", async () => {
    const seeded = orcTaskFixture("task-advance", { currentStep: 2 });
    const bridge = createMockHostBridge({ orcTasks: [seeded] });

    const advanced = await bridge.invoke("advance_orc_task", {
      taskId: "task-advance",
      kind: "instruction",
    });
    expect(advanced).toMatchObject({ state: "working", currentStep: 3 });

    // mock 写回：list 与详情同源，避免任务状态与操作结果脱节
    await expect(bridge.invoke("list_orc_tasks", {})).resolves.toMatchObject([
      { id: "task-advance", currentStep: 3 },
    ]);

    // 最后一步推进 → 汇总阶段（不直接完成，不增加 currentStep）
    const finalizing = await bridge.invoke("advance_orc_task", {
      taskId: "task-advance",
      kind: "instruction",
    });
    expect(finalizing).toMatchObject({
      currentStep: 3,
      finalizing: true,
      state: "working",
    });

    // 汇总阶段拒绝人工推进
    await expect(
      bridge.invoke("advance_orc_task", {
        taskId: "task-advance",
        kind: "instruction",
      }),
    ).rejects.toMatchObject({
      code: "orc_task_finalizing",
      message: "任务正在等待项目经理汇总，无需手动推进",
    });
  });

  it("list_orc_templates 返回三档内置模板；save 合并节点并把校验错误暴露出来", async () => {
    const bridge = createMockHostBridge();

    const templates = await bridge.invoke("list_orc_templates", {});
    expect(templates.map((template) => template.id)).toEqual([
      "template-quickfix",
      "template-standard",
      "template-full",
    ]);
    expect(templates[1].steps).toHaveLength(3);
    expect(templates[1].steps[0].agent).toBeNull();

    const saved = await bridge.invoke("save_orc_template_config", {
      templateId: "template-standard",
      steps: [
        { order: 1, agent: "opencode", model: "anthropic/claude-sonnet-4-5" },
        { order: 2, agent: "opencode", model: null },
        { order: 3, agent: null, model: null },
      ],
    });
    expect(saved[1]).toMatchObject({
      id: "template-standard",
      steps: [
        { order: 1, agent: "opencode", model: "anthropic/claude-sonnet-4-5" },
        { order: 2, agent: "opencode", model: null },
        { order: 3, agent: null, model: null },
      ],
    });

    await expect(
      bridge.invoke("save_orc_template_config", {
        templateId: "template-standard",
        steps: [
          { order: 1, agent: "opencode", model: "no-slash" },
          { order: 2, agent: null, model: null },
          { order: 3, agent: null, model: null },
        ],
      }),
    ).rejects.toMatchObject({
      code: "orc_model_invalid",
      message: "模型格式应为 provider/model：no-slash",
    });

    await expect(
      bridge.invoke("save_orc_template_config", {
        templateId: "template-standard",
        steps: [
          { order: 1, agent: "codex", model: "anthropic/claude-sonnet-4-5" },
          { order: 2, agent: null, model: null },
          { order: 3, agent: null, model: null },
        ],
      }),
    ).rejects.toMatchObject({
      code: "orc_model_agent_unsupported",
      message: "该 Agent 暂不支持指定模型",
    });
  });

  it("list_opencode_projects 返回已知项目；注入错误时明确报错", async () => {
    const bridge = createMockHostBridge();
    const projects = await bridge.invoke("list_opencode_projects", {});
    expect(projects[0]).toMatchObject({
      directory: "D:/Project/agent-notify",
      name: "agent-notify",
    });

    const failing = createMockHostBridge({
      errors: {
        list_opencode_projects: {
          code: "opencode_db_not_found",
          message: "未找到 OpenCode 项目数据库：xx（可直接手动输入工作目录）",
          retryable: false,
        },
      },
    });
    await expect(failing.invoke("list_opencode_projects", {})).rejects.toMatchObject(
      {
        code: "opencode_db_not_found",
      },
    );
  });

  it("recover 清除阻塞态（blockedStep/blockReason），回到 working", async () => {
    const blocked = orcTaskFixture("task-blocked", {
      state: "failed",
      blockedStep: 1,
      blockReason: "Step 1 投递失败：Agent 会话不可用，消息未送达。",
    });
    const bridge = createMockHostBridge({ orcTasks: [blocked] });

    const recovered = await bridge.invoke("recover_blocked_orc_task", {
      taskId: "task-blocked",
    });
    expect(recovered).toMatchObject({
      state: "working",
      blockedStep: null,
      blockReason: null,
    });
  });

  it("mark_blocked 写入失败步骤与原因；目标不存在时明确报错", async () => {
    const bridge = createMockHostBridge({
      orcTasks: [orcTaskFixture("task-live")],
    });

    const marked = await bridge.invoke("mark_blocked_orc_task", {
      taskId: "task-live",
      step: 2,
      reason: "规划 Agent 未登录，消息未送达。",
    });
    expect(marked).toMatchObject({
      state: "failed",
      blockedStep: 2,
      blockReason: "规划 Agent 未登录，消息未送达。",
    });

    await expect(
      bridge.invoke("recover_blocked_orc_task", { taskId: "missing-task" }),
    ).rejects.toMatchObject({
      code: "orc.task_not_found",
      message: "任务不存在：missing-task",
    });
  });
});
