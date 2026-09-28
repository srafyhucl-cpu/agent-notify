import { QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it } from "vitest";

import type { MockHostBridge } from "../../bridge";
import { createMockHostBridge } from "../../bridge";
import type { OrcTaskDto } from "../../bridge/types";
import { createQueryClient } from "../../data/queryClient";
import { orcTaskFixture } from "../../test/fixtures";
import { ClusterPage } from "./ClusterPage";

function renderWithQuery(bridge: MockHostBridge) {
  const queryClient = createQueryClient();
  return render(
    <QueryClientProvider client={queryClient}>
      <ClusterPage bridge={bridge} />
    </QueryClientProvider>,
  );
}

function fixturedBridge(tasks: OrcTaskDto[] = []) {
  return createMockHostBridge({ orcTasks: tasks });
}

const workingTask = orcTaskFixture("task-working", {
  goal: "把登录流程加入重试机制",
  state: "working",
  currentStep: 2,
  notifyMode: "final_only",
});

const blockedTask = orcTaskFixture("task-blocked", {
  goal: "生成周报草稿",
  state: "failed",
  currentStep: 1,
  blockedStep: 1,
  blockReason: "Step 1 投递失败：判断 Agent（Codex）会话不可用，消息未送达。",
  notifyMode: "verbose",
});

describe("ClusterPage 任务列表", () => {
  it("渲染任务的目标、状态、当前步与通知节奏；阻塞任务标红提示原因", async () => {
    const bridge = fixturedBridge([workingTask, blockedTask]);
    renderWithQuery(bridge);

    const list = await screen.findByRole("list", { name: "任务列表" });
    expect(within(list).getByText("把登录流程加入重试机制")).toBeVisible();
    expect(within(list).getByText("执行中")).toBeVisible();
    expect(within(list).getByText("第 2 步")).toBeVisible();
    expect(within(list).getByText("只推最终汇报")).toBeVisible();

    expect(within(list).getByText("生成周报草稿")).toBeVisible();
    expect(within(list).getByText("阻塞")).toBeVisible();
    expect(within(list).getByText("逐步流转")).toBeVisible();
    expect(
      within(list).getByText(/判断 Agent（Codex）会话不可用/),
    ).toBeVisible();
    expect(
      screen.getByRole("button", { name: /生成周报草稿/ }),
    ).toHaveClass("cluster-task-row--blocked");
  });

  it("无任务时显示空态，创建入口保持可用", async () => {
    const bridge = fixturedBridge();
    renderWithQuery(bridge);

    expect(
      await screen.findByRole("heading", { name: "暂无集群任务" }),
    ).toBeVisible();
    expect(
      screen.getByRole("heading", { name: "创建任务" }),
    ).toBeVisible();
  });

  it("加载中显示列表骨架，不闪占位内容", () => {
    const bridge = createMockHostBridge({
      orcTasks: [workingTask],
      delays: { list_orc_tasks: 500 },
    });
    renderWithQuery(bridge);

    expect(
      screen.getByRole("status", { name: "正在加载集群任务" }),
    ).toBeVisible();
  });

  it("读取失败展示可重试的错误态", async () => {
    const user = userEvent.setup();
    const bridge = createMockHostBridge({
      errors: {
        list_orc_tasks: {
          code: "orc_list_failed",
          message: "集群任务列表暂时不可用。",
          retryable: true,
        },
      },
    });
    renderWithQuery(bridge);

    expect(await screen.findByRole("alert")).toHaveTextContent(
      "无法读取集群任务",
    );
    expect(screen.getByText("集群任务列表暂时不可用。")).toBeVisible();

    await user.click(screen.getByRole("button", { name: "重新检查" }));
    await waitFor(() => {
      expect(bridge.calls("list_orc_tasks")).toHaveLength(2);
    });
  });
});

describe("ClusterPage 任务详情与发指令", () => {
  it("默认选中首个任务并展示状态卡与推进操作", async () => {
    const bridge = fixturedBridge([workingTask, blockedTask]);
    renderWithQuery(bridge);

    const detail = await screen.findByRole("article", {
      name: "任务详情",
    });
    expect(
      screen.getByRole("heading", { name: "把登录流程加入重试机制" }),
    ).toBeVisible();
    expect(within(detail).getByText("当前步骤")).toBeVisible();
    expect(within(detail).getAllByText("第 2 步").length).toBeGreaterThan(0);
    expect(within(detail).getByText("只推最终汇报")).toBeVisible();
    expect(within(detail).getByText("工作流节点")).toBeVisible();
    expect(within(detail).getByText("推进任务")).toBeVisible();

    for (const label of ["发指令", "确认完成", "汇报", "提问", "补充信息"]) {
      expect(
        within(detail).getByRole("button", { name: label }),
      ).toBeVisible();
    }
  });

  it("点击「发指令」调用 advance（kind=instruction）", async () => {
    const user = userEvent.setup();
    const bridge = fixturedBridge([workingTask]);
    renderWithQuery(bridge);

    const detail = await screen.findByRole("article", {
      name: "任务详情",
    });
    await user.click(within(detail).getByRole("button", { name: "发指令" }));

    await waitFor(() => {
      expect(bridge.calls("advance_orc_task")).toHaveLength(1);
    });
    expect(bridge.calls("advance_orc_task")[0]?.payload).toEqual({
      taskId: "task-working",
      kind: "instruction",
    });
  });

  it("点击「确认完成」调用 advance（kind=confirm，human_gate 语义）", async () => {
    const user = userEvent.setup();
    const bridge = fixturedBridge([workingTask]);
    renderWithQuery(bridge);

    const detail = await screen.findByRole("article", {
      name: "任务详情",
    });
    await user.click(within(detail).getByRole("button", { name: "确认完成" }));

    await waitFor(() => {
      expect(bridge.calls("advance_orc_task")).toHaveLength(1);
    });
    expect(bridge.calls("advance_orc_task")[0]?.payload).toEqual({
      taskId: "task-working",
      kind: "confirm",
    });
  });

  it("阻塞任务展示原因与「重新发起」，不提供推进操作", async () => {
    const user = userEvent.setup();
    const bridge = fixturedBridge([workingTask, blockedTask]);
    renderWithQuery(bridge);

    await screen.findByRole("list", { name: "任务列表" });
    await user.click(
      screen.getByRole("button", { name: /生成周报草稿/ }),
    );

    const alert = await screen.findByRole("alert");
    expect(alert).toHaveTextContent("任务阻塞");
    expect(alert).toHaveTextContent(/判断 Agent（Codex）会话不可用/);
    expect(screen.queryByText("推进任务")).not.toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "重新发起" }));
    await waitFor(() => {
      expect(bridge.calls("recover_blocked_orc_task")).toHaveLength(1);
    });
    expect(bridge.calls("recover_blocked_orc_task")[0]?.payload).toEqual({
      taskId: "task-blocked",
    });
  });

  it("终态任务（已完成）不再提供任何操作", async () => {
    const completed = orcTaskFixture("task-done", {
      goal: "已完成的任务",
      state: "completed",
      currentStep: 3,
    });
    const bridge = fixturedBridge([completed]);
    renderWithQuery(bridge);

    const detail = await screen.findByRole("article", {
      name: "任务详情",
    });
    expect(within(detail).getByText("已完成的任务")).toBeVisible();
    expect(within(detail).getByText("任务已结束，无待办操作。")).toBeVisible();
    expect(within(detail).queryByText("推进任务")).not.toBeInTheDocument();
    expect(
      within(detail).queryByRole("button", { name: "重新发起" }),
    ).not.toBeInTheDocument();
  });

  it("推进失败时保留页面并给出可读错误", async () => {
    const user = userEvent.setup();
    const bridge = createMockHostBridge({
      orcTasks: [workingTask],
      errors: {
        advance_orc_task: {
          code: "orc_advance_failed",
          message: "任务推进失败：调度器暂时不可用。",
          retryable: true,
        },
      },
    });
    renderWithQuery(bridge);

    const detail = await screen.findByRole("article", {
      name: "任务详情",
    });
    await user.click(within(detail).getByRole("button", { name: "发指令" }));

    expect(await screen.findByRole("alert")).toHaveTextContent(
      "任务推进失败：调度器暂时不可用。",
    );
  });
});

describe("ClusterPage 创建任务", () => {
  it("目标为空时禁用提交；填写后按所选通知节奏创建并清空表单", async () => {
    const user = userEvent.setup();
    const bridge = fixturedBridge();
    renderWithQuery(bridge);

    const submit = screen.getByRole("button", { name: "创建任务" });
    expect(submit).toBeDisabled();

    const goal = screen.getByLabelText("目标");
    await user.type(goal, "把登录流程加入重试机制");
    await user.selectOptions(screen.getByLabelText("通知节奏"), "verbose");
    expect(submit).toBeEnabled();

    await user.click(submit);
    await waitFor(() => {
      expect(bridge.calls("create_orc_task")).toHaveLength(1);
    });
    expect(bridge.calls("create_orc_task")[0]?.payload).toEqual({
      goal: "把登录流程加入重试机制",
      notifyMode: "verbose",
    });

    // 列表刷新后新任务可见，表单已清空
    const list = await screen.findByRole("list", { name: "任务列表" });
    expect(within(list).getByText("把登录流程加入重试机制")).toBeVisible();
    expect(goal).toHaveValue("");
  });

  it("默认通知节奏为 final_only，仅推最终汇报", async () => {
    const user = userEvent.setup();
    const bridge = fixturedBridge();
    renderWithQuery(bridge);

    await user.type(screen.getByLabelText("目标"), "默认节奏任务");
    await user.click(screen.getByRole("button", { name: "创建任务" }));

    await waitFor(() => {
      expect(bridge.calls("create_orc_task")).toHaveLength(1);
    });
    expect(bridge.calls("create_orc_task")[0]?.payload).toEqual({
      goal: "默认节奏任务",
      notifyMode: "final_only",
    });
  });

  it("创建失败时展示错误并保留已输入的目标", async () => {
    const user = userEvent.setup();
    const bridge = createMockHostBridge({
      errors: {
        create_orc_task: {
          code: "orc_create_failed",
          message: "任务创建失败：预置工作流不可用。",
          retryable: false,
        },
      },
    });
    renderWithQuery(bridge);

    const goal = screen.getByLabelText("目标");
    await user.type(goal, "会失败的任务");
    await user.click(screen.getByRole("button", { name: "创建任务" }));

    expect(await screen.findByRole("alert")).toHaveTextContent(
      "任务创建失败：预置工作流不可用。",
    );
    expect(screen.getByLabelText("目标")).toHaveValue("会失败的任务");
  });
});

const pendingTask = orcTaskFixture("task-pending", {
  goal: "待开始的贪吃蛇",
  state: "working",
  currentStep: 1,
  started: false,
});

describe("ClusterPage 工作流预览与开始执行", () => {
  it("创建表单展示工作流节点（每步角色与派给谁）", async () => {
    const bridge = fixturedBridge();
    renderWithQuery(bridge);

    const preview = await screen.findByLabelText("工作流节点预览");
    expect(
      within(preview).getByText("需求→判断→规划→实施（3 步）"),
    ).toBeVisible();
    expect(within(preview).getByText("初步判断")).toBeVisible();
    expect(within(preview).getByText("规划整理")).toBeVisible();
    expect(within(preview).getByText("实施")).toBeVisible();
    expect(within(preview).getByText("codex")).toBeVisible();
    expect(within(preview).getByText("opencode")).toBeVisible();
    expect(within(preview).getByText("commandcode")).toBeVisible();
  });

  it("待开始任务显示「开始执行」并调用 start_orc_task；开始后转入推进操作", async () => {
    const user = userEvent.setup();
    const bridge = fixturedBridge([pendingTask]);
    renderWithQuery(bridge);

    const detail = await screen.findByRole("article", { name: "任务详情" });
    expect(within(detail).getByText("任务待开始")).toBeVisible();

    await user.click(within(detail).getByRole("button", { name: "开始执行" }));

    await waitFor(() => {
      expect(bridge.calls("start_orc_task")).toHaveLength(1);
    });
    // mock 将 started 置 true，mutation 失效重查后转为推进操作。
    await waitFor(() => {
      expect(within(detail).getByText("推进任务")).toBeVisible();
    });
    expect(within(detail).queryByText("任务待开始")).toBeNull();
  });
});