import { QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it } from "vitest";

import type { MockHostBridge } from "../../bridge";
import { createMockHostBridge } from "../../bridge";
import type { OrcTaskDto } from "../../bridge/types";
import { createQueryClient } from "../../data/queryClient";
import {
  opencodeProjectsFixture,
  orcTaskFixture,
  unconfiguredOrcTaskFixture,
} from "../../test/fixtures";
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

/** 选择模板 + 工作目录，让创建按钮可用（多数创建用例的公共前置）。 */
async function fillRequiredFields(
  user: ReturnType<typeof userEvent.setup>,
  options: { goal: string; directory?: string },
) {
  // 模板与项目列表是异步查询：先等到选项就绪再选择。
  await screen.findByRole("option", { name: "标准交付（3 步）" });
  await screen.findByRole("option", { name: /agent-notify/ });
  await user.type(screen.getByLabelText("目标"), options.goal);
  await user.selectOptions(
    screen.getByLabelText("工作流模板"),
    "template-standard",
  );
  await user.selectOptions(
    screen.getByLabelText("工作目录"),
    options.directory ?? "D:/Project/agent-notify",
  );
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

  it("汇总中的任务在列表卡片标注「汇总中」", async () => {
    const finalizingTask = orcTaskFixture("task-finalizing", {
      goal: "等待项目经理汇总",
      state: "working",
      currentStep: 3,
      finalizing: true,
    });
    renderWithQuery(fixturedBridge([finalizingTask]));

    const list = await screen.findByRole("list", { name: "任务列表" });
    expect(within(list).getByText("汇总中")).toBeVisible();
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

  it("事实区展示工作目录；旧任务缺工作目录时明确说明跟随宿主", async () => {
    const legacyTask = orcTaskFixture("task-legacy", {
      goal: "旧任务",
      workingDir: null,
    });
    renderWithQuery(fixturedBridge([workingTask, legacyTask]));

    const detail = await screen.findByRole("article", { name: "任务详情" });
    expect(within(detail).getByText("工作目录")).toBeVisible();
    expect(
      within(detail).getByText("D:/Project/agent-notify"),
    ).toBeVisible();

    await userEvent.click(screen.getByRole("button", { name: /旧任务/ }));
    expect(
      await within(detail).findByText("跟随宿主当前项目"),
    ).toBeVisible();
  });

  it("节点链展示用途/Agent/模型与当前节点状态标签", async () => {
    renderWithQuery(fixturedBridge([workingTask]));

    const chain = await screen.findByRole("list", { name: "工作流节点" });
    // 首节点 = 项目经理；未配置/已配置 Agent 都直接可读
    expect(within(chain).getByText("项目经理")).toBeVisible();
    expect(within(chain).getByText("初步判断")).toBeVisible();
    expect(within(chain).getByText("规划整理")).toBeVisible();
    expect(within(chain).getByText("实施")).toBeVisible();
    expect(within(chain).getByText("codex")).toBeVisible();
    expect(within(chain).getByText("opencode")).toBeVisible();
    // 配置了模型的节点显示模型，未配置的显示默认模型（透明语义）
    expect(
      within(chain).getByText("anthropic/claude-sonnet-4-5"),
    ).toBeVisible();
    expect(within(chain).getAllByText("默认模型").length).toBeGreaterThan(0);
    // 状态语义由中文标签承载（动效只做引导）
    expect(within(chain).getByText("当前节点")).toBeVisible();
    expect(within(chain).getByText("已完成")).toBeVisible();
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

  it("汇总阶段：首节点回到脉冲态并标注「项目经理汇总中」，隐藏推进操作", async () => {
    const finalizingTask = orcTaskFixture("task-finalizing", {
      goal: "汇总阶段任务",
      state: "working",
      currentStep: 3,
      finalizing: true,
    });
    renderWithQuery(fixturedBridge([finalizingTask]));

    const detail = await screen.findByRole("article", { name: "任务详情" });
    expect(
      within(detail).getByText("项目经理正在汇总，等待最终汇报；汇总完成后任务自动结束。"),
    ).toBeVisible();
    expect(screen.queryByText("推进任务")).not.toBeInTheDocument();
    expect(
      within(detail).queryByRole("button", { name: "发指令" }),
    ).not.toBeInTheDocument();
    // 首节点回到脉冲态（动效 class 的载体），去掉动效仍保留文字语义
    const chain = within(detail).getByRole("list", { name: "工作流节点" });
    expect(chain.querySelector('[data-state="finalizing"]')).not.toBeNull();
    expect(within(chain).getByText("项目经理汇总中")).toBeVisible();
  });

  it("手动推进到汇总阶段后再点推进会展示后端中文拒绝原因", async () => {
    const user = userEvent.setup();
    const task = orcTaskFixture("task-last-step", {
      goal: "最后一步任务",
      currentStep: 3,
    });
    const bridge = fixturedBridge([task]);
    renderWithQuery(bridge);

    const detail = await screen.findByRole("article", { name: "任务详情" });
    await user.click(within(detail).getByRole("button", { name: "发指令" }));

    // mock：最后一步推进 → finalizing；再推进 → orc_task_finalizing
    expect(
      await screen.findByText("项目经理正在汇总，等待最终汇报；汇总完成后任务自动结束。"),
    ).toBeVisible();
  });

  it("未配置 Agent 的任务点「开始执行」时展示后端预检中文错误", async () => {
    const user = userEvent.setup();
    const task = unconfiguredOrcTaskFixture("task-unconfigured");
    const bridge = fixturedBridge([task]);
    renderWithQuery(bridge);

    const detail = await screen.findByRole("article", { name: "任务详情" });
    await user.click(within(detail).getByRole("button", { name: "开始执行" }));

    expect(await screen.findByRole("alert")).toHaveTextContent(
      "第 1 步未选择 Agent：请先在设置 → 编排中配置",
    );
  });
});

const pendingTask = orcTaskFixture("task-pending", {
  goal: "待开始的贪吃蛇",
  state: "working",
  currentStep: 1,
  started: false,
});

describe("ClusterPage 创建任务", () => {
  it("目标/模板/工作目录齐全后才能提交；提交后清空表单并刷新列表", async () => {
    const user = userEvent.setup();
    const bridge = fixturedBridge();
    renderWithQuery(bridge);

    const submit = screen.getByRole("button", { name: "创建任务" });
    expect(submit).toBeDisabled();

    // 模板与项目列表就绪后再交互
    await screen.findByRole("option", { name: "标准交付（3 步）" });
    await screen.findByRole("option", { name: /agent-notify/ });

    const goal = screen.getByLabelText("目标");
    await user.type(goal, "把登录流程加入重试机制");
    expect(submit).toBeDisabled();

    // 模板必选（无默认）
    await user.selectOptions(
      screen.getByLabelText("工作流模板"),
      "template-standard",
    );
    expect(submit).toBeDisabled();

    // 工作目录必填
    await user.selectOptions(screen.getByLabelText("通知节奏"), "verbose");
    await user.selectOptions(
      screen.getByLabelText("工作目录"),
      "D:/Project/agent-notify",
    );
    expect(submit).toBeEnabled();

    await user.click(submit);
    await waitFor(() => {
      expect(bridge.calls("create_orc_task")).toHaveLength(1);
    });
    expect(bridge.calls("create_orc_task")[0]?.payload).toEqual({
      goal: "把登录流程加入重试机制",
      notifyMode: "verbose",
      templateId: "template-standard",
      workingDir: "D:/Project/agent-notify",
    });

    // 列表刷新后新任务可见，表单已清空
    const list = await screen.findByRole("list", { name: "任务列表" });
    expect(within(list).getByText("把登录流程加入重试机制")).toBeVisible();
    expect(goal).toHaveValue("");
    expect(screen.getByLabelText("工作流模板")).toHaveValue("");
    expect(screen.getByLabelText("工作目录")).toHaveValue("");
  });

  it("默认通知节奏为 final_only，仅推最终汇报", async () => {
    const user = userEvent.setup();
    const bridge = fixturedBridge();
    renderWithQuery(bridge);

    await fillRequiredFields(user, { goal: "默认节奏任务" });
    await user.click(screen.getByRole("button", { name: "创建任务" }));

    await waitFor(() => {
      expect(bridge.calls("create_orc_task")).toHaveLength(1);
    });
    expect(bridge.calls("create_orc_task")[0]?.payload).toEqual({
      goal: "默认节奏任务",
      notifyMode: "final_only",
      templateId: "template-standard",
      workingDir: "D:/Project/agent-notify",
    });
  });

  it("工作目录下拉按最近活跃倒序展示「名称（目录）」，无名称只显示目录", async () => {
    renderWithQuery(fixturedBridge());

    await screen.findByRole("option", { name: /agent-notify/ });
    const select = screen.getByLabelText("工作目录");
    const options = within(select)
      .getAllByRole("option")
      .map((option) => option.textContent);
    expect(options).toEqual([
      "请选择工作目录",
      "agent-notify（D:/Project/agent-notify）",
      "D:/Project/legacy-demo",
      "手动输入",
    ]);
  });

  it("选择「手动输入」后展示文本框，提交使用手填目录", async () => {
    const user = userEvent.setup();
    const bridge = fixturedBridge();
    renderWithQuery(bridge);

    expect(
      screen.queryByLabelText("工作目录（手动输入）"),
    ).not.toBeInTheDocument();

    await fillRequiredFields(user, { goal: "手动目录任务" });
    await user.selectOptions(screen.getByLabelText("工作目录"), "__manual__");
    const manual = await screen.findByLabelText("工作目录（手动输入）");
    await user.type(manual, "D:/Project/manual-app");
    await user.click(screen.getByRole("button", { name: "创建任务" }));

    await waitFor(() => {
      expect(bridge.calls("create_orc_task")).toHaveLength(1);
    });
    expect(bridge.calls("create_orc_task")[0]?.payload).toMatchObject({
      workingDir: "D:/Project/manual-app",
    });
  });

  it("项目列表读取失败：明确报错并退回手动输入（不静默）", async () => {
    const user = userEvent.setup();
    const bridge = createMockHostBridge({
      errors: {
        list_opencode_projects: {
          code: "opencode_db_not_found",
          message:
            "未找到 OpenCode 项目数据库：C:/Users/demo/.local/share/opencode/opencode.db（可直接手动输入工作目录）",
          retryable: false,
        },
      },
    });
    renderWithQuery(bridge);

    expect(await screen.findByRole("alert")).toHaveTextContent(
      "无法读取 OpenCode 项目",
    );
    expect(
      screen.getByText(/未找到 OpenCode 项目数据库/),
    ).toBeVisible();
    // 自动退回手动输入，且校验仍要求填写目录
    expect(screen.getByLabelText("工作目录（手动输入）")).toBeVisible();
    expect(screen.getByRole("button", { name: "创建任务" })).toBeDisabled();

    await user.click(screen.getByRole("button", { name: "重新读取" }));
    await waitFor(() => {
      expect(bridge.calls("list_opencode_projects")).toHaveLength(2);
    });
  });

  it("未读取到项目时提示手动输入（不静默使用空下拉）", async () => {
    const bridge = createMockHostBridge({ opencodeProjects: [] });
    renderWithQuery(bridge);

    expect(
      await screen.findByText("未读取到 OpenCode 项目，请手动输入工作目录。"),
    ).toBeVisible();
    expect(screen.getByLabelText("工作目录（手动输入）")).toBeVisible();
  });

  it("创建失败时展示后端中文错误并保留已输入的目标", async () => {
    const user = userEvent.setup();
    const bridge = createMockHostBridge({
      errors: {
        create_orc_task: {
          code: "orc_working_dir_invalid",
          message: "工作目录不存在：D:/Project/gone",
          retryable: false,
        },
      },
    });
    renderWithQuery(bridge);

    const goal = screen.getByLabelText("目标");
    await user.type(goal, "会失败的任务");
    await user.selectOptions(
      screen.getByLabelText("工作流模板"),
      "template-standard",
    );
    await user.selectOptions(screen.getByLabelText("工作目录"), "__manual__");
    await user.type(
      screen.getByLabelText("工作目录（手动输入）"),
      "D:/Project/gone",
    );
    await user.click(screen.getByRole("button", { name: "创建任务" }));

    expect(await screen.findByRole("alert")).toHaveTextContent(
      "工作目录不存在：D:/Project/gone",
    );
    expect(screen.getByLabelText("目标")).toHaveValue("会失败的任务");
  });
});

describe("ClusterPage 工作流预览与开始执行", () => {
  it("选中模板后展示节点链（用途/项目经理/未配置/默认模型）", async () => {
    const user = userEvent.setup();
    const bridge = fixturedBridge();
    renderWithQuery(bridge);

    await screen.findByRole("option", { name: "标准交付（3 步）" });
    await user.selectOptions(
      screen.getByLabelText("工作流模板"),
      "template-standard",
    );

    const preview = await screen.findByRole("list", {
      name: "工作流节点预览",
    });
    // 标题与节点链同属预览区（下拉选项同名，用预览容器断言）
    expect(preview.closest(".cluster-workflow-preview")).toHaveTextContent(
      "标准交付（3 步）",
    );
    expect(within(preview).getByText("规划整理")).toBeVisible();
    expect(within(preview).getByText("实施")).toBeVisible();
    expect(within(preview).getByText("复核汇总")).toBeVisible();
    expect(within(preview).getByText("项目经理")).toBeVisible();
    expect(within(preview).getAllByText("未配置").length).toBe(3);
    expect(within(preview).getAllByText("默认模型").length).toBe(3);
  });

  it("模板读取失败时展示错误并禁用模板选择", async () => {
    const user = userEvent.setup();
    const bridge = createMockHostBridge({
      errors: {
        list_orc_templates: {
          code: "orc_templates_unavailable",
          message: "模板列表暂时不可用。",
          retryable: true,
        },
      },
    });
    renderWithQuery(bridge);

    expect(await screen.findByRole("alert")).toHaveTextContent(
      "无法读取工作流模板",
    );
    expect(screen.getByText("模板列表暂时不可用。")).toBeVisible();
    expect(screen.getByLabelText("工作流模板")).toBeDisabled();
    expect(screen.getByRole("button", { name: "创建任务" })).toBeDisabled();

    await user.click(screen.getByRole("button", { name: "重新读取" }));
    await waitFor(() => {
      expect(bridge.calls("list_orc_templates")).toHaveLength(2);
    });
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

  it("已配置项目样本按最近活跃倒序（fixture 契约）", () => {
    const projects = opencodeProjectsFixture();
    expect(projects[0]?.name).toBe("agent-notify");
    expect(projects[1]?.name).toBeNull();
  });
});
