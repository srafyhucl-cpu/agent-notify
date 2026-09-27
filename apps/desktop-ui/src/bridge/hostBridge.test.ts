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
      notifyMode: "verbose",
    });
    expect(created).toMatchObject({
      id: "orc-2",
      state: "working",
      currentStep: 1,
      notifyMode: "verbose",
      goal: "新任务",
    });

    await expect(bridge.invoke("list_orc_tasks", {})).resolves.toHaveLength(2);
  });

  it("create 未传 notify_mode 时回落默认 final_only", async () => {
    const bridge = createMockHostBridge();
    const created = await bridge.invoke("create_orc_task", {
      goal: "默认节奏",
      notifyMode: null,
    });
    expect(created.notifyMode).toBe("final_only");
  });

  it("advance 写回推进 currentStep（上限 3）且列表同步", async () => {
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
    // 到顶后不再推进
    await bridge.invoke("advance_orc_task", {
      taskId: "task-advance",
      kind: "instruction",
    });
    await expect(bridge.invoke("list_orc_tasks", {})).resolves.toMatchObject([
      { id: "task-advance", currentStep: 3 },
    ]);
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
