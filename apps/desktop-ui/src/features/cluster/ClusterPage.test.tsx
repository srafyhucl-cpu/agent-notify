import { QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it } from "vitest";

import type { MockHostBridge } from "../../bridge";
import { createMockHostBridge } from "../../bridge";
import type { OrcTaskDto } from "../../bridge/types";
import { createQueryClient } from "../../data/queryClient";
import {
  agentFixture,
  configuredTemplateFixture,
  opencodeProjectsFixture,
  orcTaskFixture,
  orcTemplatesFixture,
  twoAgents,
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
  return createMockHostBridge({ agents: twoAgents, orcTasks: tasks });
}

const workingTask = orcTaskFixture("task-working", {
  goal: "把登录流程加入重试机制",
  name: "登录重试",
  state: "working",
  currentStep: 2,
  notifyMode: "final_only",
});

const blockedTask = orcTaskFixture("task-blocked", {
  goal: "生成周报草稿",
  name: "周报草稿",
  state: "failed",
  currentStep: 1,
  blockedStep: 1,
  blockReason: "Step 1 投递失败：判断 Agent（Codex）会话不可用，消息未送达。",
  notifyMode: "verbose",
});

const pendingTask = orcTaskFixture("task-pending", {
  goal: "待开始的贪吃蛇",
  name: "贪吃蛇",
  state: "working",
  currentStep: 1,
  started: false,
});

const completedTask = orcTaskFixture("task-done", {
  goal: "已完成的任务",
  name: "已完成",
  state: "completed",
  currentStep: 3,
});

function taskTable() {
  return screen.findByRole("table", { name: "编排任务列表" });
}

/** 点任务行展开手风琴（默认全部收起）：按任务名称定位。 */
async function expandTask(
  user: ReturnType<typeof userEvent.setup>,
  name: string,
) {
  const list = await taskTable();
  await user.click(within(list).getByRole("button", { name }));
  return screen.findByRole("article", { name: "任务详情" });
}

async function openCreateDialog(user: ReturnType<typeof userEvent.setup>) {
  await user.click(screen.getByRole("button", { name: "新建任务" }));
  return screen.findByRole("dialog", { name: "新建编排任务" });
}

/**
 * 弹窗创建公共前置：目标 + 模板 + 工作目录 + 每个节点选 Agent。
 * 模板节点默认不预填 Agent，必须逐个选择（未选齐不能提交）。
 */
async function fillCreateDialog(
  user: ReturnType<typeof userEvent.setup>,
  options: {
    goal: string;
    name?: string;
    orderCount?: number;
    agent?: string;
    directory?: string;
    manualDir?: string;
  },
) {
  const dialog = await openCreateDialog(user);
  await user.type(
    within(dialog).getByLabelText("任务名称"),
    options.name ?? "测试任务",
  );
  await user.type(within(dialog).getByLabelText("任务描述"), options.goal);
  await user.selectOptions(
    within(dialog).getByLabelText("工作流模板"),
    "template-standard",
  );
  if (options.manualDir) {
    await user.selectOptions(
      within(dialog).getByLabelText("工作目录"),
      "__manual__",
    );
    await user.type(
      await within(dialog).findByLabelText("工作目录（手动输入）"),
      options.manualDir,
    );
  } else {
    await user.selectOptions(
      within(dialog).getByLabelText("工作目录"),
      options.directory ?? "D:/Project/agent-notify",
    );
  }
  await within(dialog).findAllByRole("option", { name: /Alpha Agent/ });
  for (let order = 1; order <= (options.orderCount ?? 3); order++) {
    await user.selectOptions(
      within(dialog).getByLabelText(`第 ${order} 步 Agent`),
      options.agent ?? "alpha",
    );
  }
  return dialog;
}

describe("ClusterPage 任务列表", () => {
  it("行内展示任务名称+描述、状态、进度段与通知节奏；阻塞任务显示原因并标红", async () => {
    renderWithQuery(fixturedBridge([workingTask, blockedTask]));

    const list = await taskTable();
    // 行内只展示名称（描述只在展开详情里）；名称可点开详情
    expect(within(list).getByRole("button", { name: "登录重试" })).toBeVisible();
    expect(within(list).queryByText("把登录流程加入重试机制")).not.toBeInTheDocument();
    expect(within(list).getByText("执行中")).toBeVisible();
    expect(within(list).getByText("只推最终汇报")).toBeVisible();
    // 进度段：3 步 = 3 段（当前/完成/待执行用样式区分）
    const workingRow = within(list)
      .getByRole("button", { name: "登录重试" })
      .closest(".cluster-task-row");
    expect(workingRow?.querySelectorAll(".cluster-step-dot")).toHaveLength(3);
    expect(workingRow?.querySelector(".cluster-step-dot--current")).not.toBeNull();
    expect(workingRow?.querySelector(".cluster-step-dot--done")).not.toBeNull();

    expect(within(list).getByRole("button", { name: "周报草稿" })).toBeVisible();
    expect(within(list).getByText("阻塞")).toBeVisible();
    expect(within(list).getByText("逐步流转")).toBeVisible();
    // 报错不在列表里堆：原因只在展开详情中展示
    expect(
      within(list).queryByText(/判断 Agent（Codex）会话不可用/),
    ).not.toBeInTheDocument();
    const blockedRow = within(list)
      .getByRole("button", { name: "周报草稿" })
      .closest(".cluster-task-row");
    expect(blockedRow).toHaveClass("cluster-task-row--blocked");
    expect(blockedRow?.querySelector(".cluster-step-dot--failed")).not.toBeNull();
  });

  it("未开始的任务显示「待开始」而不是「执行中」", async () => {
    renderWithQuery(fixturedBridge([pendingTask]));

    const list = await taskTable();
    expect(within(list).getByText("待开始")).toBeVisible();
    expect(within(list).queryByText("执行中")).not.toBeInTheDocument();
  });

  it("汇总中的任务行标注「汇总中」", async () => {
    const finalizingTask = orcTaskFixture("task-finalizing", {
      goal: "等待项目经理汇总",
      state: "working",
      currentStep: 3,
      finalizing: true,
    });
    renderWithQuery(fixturedBridge([finalizingTask]));

    const list = await taskTable();
    expect(within(list).getByText("汇总中")).toBeVisible();
  });

  it("进入页面不默认展开任何任务；点行展开、再点收起", async () => {
    const user = userEvent.setup();
    renderWithQuery(fixturedBridge([workingTask, blockedTask]));

    await taskTable();
    expect(
      screen.queryByRole("article", { name: "任务详情" }),
    ).not.toBeInTheDocument();

    await expandTask(user, "登录重试");
    expect(
      screen.getByRole("article", { name: "任务详情" }),
    ).toBeVisible();

    const list = await taskTable();
    await user.click(within(list).getByRole("button", { name: "登录重试" }));
    expect(
      screen.queryByRole("article", { name: "任务详情" }),
    ).not.toBeInTheDocument();
  });

  it("展开其他任务时手风琴只保留一个详情", async () => {
    const user = userEvent.setup();
    renderWithQuery(fixturedBridge([workingTask, blockedTask]));

    await expandTask(user, "登录重试");
    await expandTask(user, "周报草稿");

    expect(screen.getAllByRole("article", { name: "任务详情" })).toHaveLength(1);
    const detail = screen.getByRole("article", { name: "任务详情" });
    expect(within(detail).getByText("任务阻塞")).toBeVisible();
  });

  it("无任务时显示空态，右上角保留「新建任务」入口", async () => {
    renderWithQuery(fixturedBridge());

    expect(
      await screen.findByRole("heading", { name: "暂无集群任务" }),
    ).toBeVisible();
    expect(screen.getByRole("button", { name: "新建任务" })).toBeEnabled();
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
  it("展开后按块展示：描述 / 工作流 / 任务信息（推进操作跟随当前节点；不重复名称）", async () => {
    const user = userEvent.setup();
    renderWithQuery(fixturedBridge([workingTask]));

    const detail = await expandTask(user, "登录重试");
    // 分块：每块一个带标题的区块（描述、工作流、任务信息）
    for (const pane of ["任务描述", "工作流", "任务信息"]) {
      expect(within(detail).getByRole("region", { name: pane })).toBeVisible();
    }
    // 推进操作集成在工作流当前节点卡内（不再单开「操作」区块）
    expect(
      within(detail).queryByRole("region", { name: "操作" }),
    ).not.toBeInTheDocument();
    // 名称只在列表行出现，详情里不重复标题
    expect(
      within(detail).queryByRole("heading", { name: "登录重试" }),
    ).not.toBeInTheDocument();
    expect(within(detail).getByText("把登录流程加入重试机制")).toBeVisible();

    const chain = within(detail).getByRole("list", { name: "工作流节点" });
    // 首节点 = 项目经理；用途/Agent/模型都可读
    expect(within(chain).getByText("项目经理")).toBeVisible();
    expect(within(chain).getByText("规划整理")).toBeVisible();
    expect(within(chain).getByText("codex")).toBeVisible();
    expect(within(chain).getByText("opencode")).toBeVisible();
    expect(
      within(chain).getByText("anthropic/claude-sonnet-4-5"),
    ).toBeVisible();
    // 未指定模型：显示「未指定」，不再写含糊的「默认模型」
    expect(within(chain).getAllByText("未指定").length).toBeGreaterThan(0);
    expect(within(chain).queryByText("默认模型")).not.toBeInTheDocument();
    // 状态语义由中文标签承载（动效只做引导）
    expect(within(chain).getByText("当前节点")).toBeVisible();
    expect(within(chain).getByText("已完成")).toBeVisible();

    expect(within(detail).getByText("当前步骤")).toBeVisible();
    expect(within(detail).getAllByText("第 2 步").length).toBeGreaterThan(0);
    expect(within(detail).getByText("只推最终汇报")).toBeVisible();
    expect(
      within(detail).getByText("D:/Project/agent-notify"),
    ).toBeVisible();
    for (const label of ["发指令", "汇报"]) {
      expect(within(detail).getByRole("button", { name: label })).toBeVisible();
    }
  });

  it("旧任务缺工作目录时明确说明跟随宿主当前项目", async () => {
    const user = userEvent.setup();
    const legacyTask = orcTaskFixture("task-legacy", {
      goal: "旧任务",
      workingDir: null,
    });
    renderWithQuery(fixturedBridge([legacyTask]));

    const detail = await expandTask(user, "旧任务");
    expect(within(detail).getByText("跟随宿主当前项目")).toBeVisible();
  });

  it("点击「发指令」调用 advance（kind=instruction）", async () => {
    const user = userEvent.setup();
    const bridge = fixturedBridge([workingTask]);
    renderWithQuery(bridge);

    const detail = await expandTask(user, "登录重试");
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
    const gateTask = orcTaskFixture("task-gate-click", {
      goal: "确认门点击",
      name: "确认门点",
      state: "input_required",
      currentStep: 2,
    });
    const bridge = fixturedBridge([gateTask]);
    renderWithQuery(bridge);

    const detail = await expandTask(user, "确认门点");
    await user.click(within(detail).getByRole("button", { name: "确认完成" }));

    await waitFor(() => {
      expect(bridge.calls("advance_orc_task")).toHaveLength(1);
    });
    expect(bridge.calls("advance_orc_task")[0]?.payload).toEqual({
      taskId: "task-gate-click",
      kind: "confirm",
    });
  });

  it("阻塞任务展示原因与「重新发起」，不提供推进操作", async () => {
    const user = userEvent.setup();
    const bridge = fixturedBridge([blockedTask]);
    renderWithQuery(bridge);

    const detail = await expandTask(user, "周报草稿");
    const alert = within(detail).getByRole("alert");
    expect(alert).toHaveTextContent("任务阻塞");
    expect(alert).toHaveTextContent(/判断 Agent（Codex）会话不可用/);
    // 旧数据里的「Step N」展示成中文「第 N 步」
    expect(alert).toHaveTextContent("第 1 步投递失败");
    expect(within(detail).queryByRole("region", { name: "操作" })).not.toBeInTheDocument();

    await user.click(within(detail).getByRole("button", { name: "重新发起" }));
    await waitFor(() => {
      expect(bridge.calls("recover_blocked_orc_task")).toHaveLength(1);
    });
    expect(bridge.calls("recover_blocked_orc_task")[0]?.payload).toEqual({
      taskId: "task-blocked",
    });
  });

  it("旧任务的英文模型报错在详情里翻译成中文处理建议（不含 ID/英文原文）", async () => {
    const user = userEvent.setup();
    const task = orcTaskFixture("task-model-fail", {
      goal: "模型失败任务",
      name: "模型失败",
      state: "failed",
      currentStep: 1,
      blockedStep: 1,
      blockReason:
        "Step 1 执行失败：Model unavailable: provider/DeepSeek V4.1 Flash",
    });
    renderWithQuery(fixturedBridge([task]));

    const detail = await expandTask(user, "模型失败");
    const alert = within(detail).getByRole("alert");
    expect(alert).toHaveTextContent("所选模型不可用");
    expect(alert).toHaveTextContent("换一个模型后点「重新发起」");
    expect(alert).not.toHaveTextContent("Model unavailable");
    expect(alert).not.toHaveTextContent("provider/");
  });

  it("终态任务（已完成）提供「继续迭代」，不提供推进操作", async () => {
    const user = userEvent.setup();
    renderWithQuery(fixturedBridge([completedTask]));

    const detail = await expandTask(user, "已完成");
    expect(
      within(detail).getByRole("button", { name: "继续迭代（第 2 轮）" }),
    ).toBeVisible();
    expect(
      within(detail).queryByRole("button", { name: "发指令" }),
    ).not.toBeInTheDocument();
    expect(
      within(detail).queryByRole("button", { name: "重新发起" }),
    ).not.toBeInTheDocument();
  });

  it("「继续迭代」：填写本轮要求后开始新一轮（提交 instruction，列表出现第 2 轮）", async () => {
    const user = userEvent.setup();
    const bridge = fixturedBridge([completedTask]);
    renderWithQuery(bridge);

    const detail = await expandTask(user, "已完成");
    await user.click(
      within(detail).getByRole("button", { name: "继续迭代（第 2 轮）" }),
    );
    const dialog = await screen.findByRole("dialog", {
      name: "继续迭代（第 2 轮）",
    });
    await user.type(
      within(dialog).getByLabelText("本轮要求"),
      "翅膀握住车把，腿自然弯曲",
    );
    await user.click(
      within(dialog).getByRole("button", { name: "开始新一轮" }),
    );

    await waitFor(() => {
      expect(bridge.calls("continue_orc_task")).toHaveLength(1);
    });
    expect(bridge.calls("continue_orc_task")[0]?.payload).toEqual({
      taskId: "task-done",
      instruction: "翅膀握住车把，腿自然弯曲",
    });
    // mock：round+1、回到第 1 步 → 列表出现「第 2 轮」徽标、状态回到执行中
    const list = await taskTable();
    await waitFor(() => {
      expect(within(list).getAllByText("第 2 轮").length).toBeGreaterThan(0);
    });
  });

  it("「继续迭代」留空：instruction 为 null（交给项目经理按上一轮结论继续）", async () => {
    const user = userEvent.setup();
    const bridge = fixturedBridge([completedTask]);
    renderWithQuery(bridge);

    const detail = await expandTask(user, "已完成");
    await user.click(
      within(detail).getByRole("button", { name: "继续迭代（第 2 轮）" }),
    );
    const dialog = await screen.findByRole("dialog", {
      name: "继续迭代（第 2 轮）",
    });
    await user.click(
      within(dialog).getByRole("button", { name: "开始新一轮" }),
    );

    await waitFor(() => {
      expect(bridge.calls("continue_orc_task")).toHaveLength(1);
    });
    expect(bridge.calls("continue_orc_task")[0]?.payload).toEqual({
      taskId: "task-done",
      instruction: null,
    });
  });

  it("第 2 轮任务：列表显示轮次徽标，详情叠放轮次卡片并可展开完整时间线", async () => {
    const user = userEvent.setup();
    const roundTwo = orcTaskFixture("task-round2", {
      goal: "鹈鹕骑车图",
      name: "鹈鹕迭代",
      round: 2,
      roundInput: "翅膀握住车把，腿自然弯曲",
      roundHistory: [
        {
          round: 1,
          input: null,
          summary: "第 1 轮结论：翅膀角度偏硬，腿太直。",
        },
        {
          round: 2,
          input: "翅膀握住车把，腿自然弯曲",
          summary: null,
        },
      ],
    });
    renderWithQuery(fixturedBridge([roundTwo]));

    const list = await taskTable();
    expect(within(list).getByText("第 2 轮")).toBeVisible();

    const detail = await expandTask(user, "鹈鹕迭代");
    expect(within(detail).getByText("轮次")).toBeVisible();
    // 折叠态：叠放卡片露出最新一轮要求 + 展开入口。
    expect(
      within(detail).getByText("翅膀握住车把，腿自然弯曲"),
    ).toBeVisible();
    await user.click(
      within(detail).getByRole("button", { name: /展开迭代时间线/ }),
    );

    // 展开态：完整时间线（第 1 轮初始需求 + 结论；第 2 轮本轮要求 + 进行中）。
    expect(within(detail).getByText("迭代时间线")).toBeVisible();
    expect(within(detail).getByText("初始需求")).toBeVisible();
    expect(
      within(detail).getByText("第 1 轮结论：翅膀角度偏硬，腿太直。"),
    ).toBeVisible();
    expect(within(detail).getByText("本轮要求")).toBeVisible();
    expect(within(detail).getByText("本轮进行中")).toBeVisible();
    // 收起入口与展开入口同位置（右上角），点它回到叠放态。
    await user.click(
      within(detail).getByRole("button", { name: /收起迭代时间线/ }),
    );
    expect(
      within(detail).getByRole("button", { name: /展开迭代时间线/ }),
    ).toBeVisible();
  });

  it("创建时间：列表行显示短格式，详情信息一行显示完整格式", async () => {
    const user = userEvent.setup();
    const task = orcTaskFixture("task-created", {
      goal: "展示创建时间",
      createdAt: "2026-09-29T08:25:02.964Z",
    });
    renderWithQuery(fixturedBridge([task]));

    const list = await taskTable();
    const short = within(list).getByText(/^\d{2}-\d{2} \d{2}:\d{2}$/);
    expect(short).toBeVisible();
    expect(short.getAttribute("title")).toMatch(
      /^创建于 \d{4}-\d{2}-\d{2} \d{2}:\d{2}$/,
    );

    const detail = await expandTask(user, "展示创建时间");
    expect(within(detail).getByText("创建于")).toBeVisible();
    expect(
      within(detail).getByText(/^\d{4}-\d{2}-\d{2} \d{2}:\d{2}$/),
    ).toBeVisible();
  });

  it("操作按钮带一句话悬浮说明（title）", async () => {
    const user = userEvent.setup();
    renderWithQuery(fixturedBridge([workingTask]));

    const detail = await expandTask(user, "登录重试");
    expect(
      within(detail)
        .getByRole("button", { name: "发指令" })
        .getAttribute("title"),
    ).toContain("补充要求");
    expect(
      within(detail)
        .getByRole("button", { name: "汇报" })
        .getAttribute("title"),
    ).toContain("推进到下一步");
  });

  it("等待人工确认：只给「确认完成」与「发指令」（随节点状态切换）", async () => {
    const user = userEvent.setup();
    const gateTask = orcTaskFixture("task-gate", {
      goal: "人工确认门",
      name: "确认门",
      state: "input_required",
      currentStep: 2,
    });
    renderWithQuery(fixturedBridge([gateTask]));

    const detail = await expandTask(user, "确认门");
    expect(
      within(detail)
        .getByRole("button", { name: "确认完成" })
        .getAttribute("title"),
    ).toContain("人工确认门");
    // 干活态才有的「汇报」在确认门阶段隐藏
    expect(
      within(detail).queryByRole("button", { name: "汇报" }),
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

    const detail = await expandTask(user, "登录重试");
    await user.click(within(detail).getByRole("button", { name: "发指令" }));

    expect(await screen.findByRole("alert")).toHaveTextContent(
      "任务推进失败：调度器暂时不可用。",
    );
  });

  it("汇总阶段：首节点回到脉冲态并标注「项目经理汇总中」，隐藏推进操作", async () => {
    const user = userEvent.setup();
    const finalizingTask = orcTaskFixture("task-finalizing", {
      goal: "汇总阶段任务",
      name: "汇总阶段",
      state: "working",
      currentStep: 3,
      finalizing: true,
    });
    renderWithQuery(fixturedBridge([finalizingTask]));

    const detail = await expandTask(user, "汇总阶段");
    expect(
      within(detail).getByText(
        "项目经理正在汇总，等待最终汇报；汇总完成后任务自动结束。",
      ),
    ).toBeVisible();
    expect(
      within(detail).queryByRole("button", { name: "发指令" }),
    ).not.toBeInTheDocument();
    const chain = within(detail).getByRole("list", { name: "工作流节点" });
    expect(chain.querySelector('[data-state="finalizing"]')).not.toBeNull();
    expect(within(chain).getByText("项目经理汇总中")).toBeVisible();
  });

  it("未配置 Agent 的任务点「开始执行」时展示后端预检中文错误", async () => {
    const user = userEvent.setup();
    const task = {
      ...unconfiguredOrcTaskFixture("task-unconfigured"),
      name: "未配置",
    };
    renderWithQuery(fixturedBridge([task]));

    const detail = await expandTask(user, "未配置");
    await user.click(within(detail).getByRole("button", { name: "开始执行" }));

    expect(await screen.findByRole("alert")).toHaveTextContent(
      "第 1 步未选择 Agent：请先在设置 → 编排中配置",
    );
  });

  it("待开始任务显示「开始执行」并调用 start_orc_task；开始后转入推进操作", async () => {
    const user = userEvent.setup();
    const bridge = fixturedBridge([pendingTask]);
    renderWithQuery(bridge);

    const detail = await expandTask(user, "贪吃蛇");
    expect(within(detail).getByRole("region", { name: "工作流" })).toBeVisible();
    expect(within(detail).getByText("待开始")).toBeVisible();

    await user.click(within(detail).getByRole("button", { name: "开始执行" }));

    await waitFor(() => {
      expect(bridge.calls("start_orc_task")).toHaveLength(1);
    });
    // mock 将 started 置 true，mutation 失效重查后转为推进操作。
    await waitFor(() => {
      expect(within(detail).getByRole("button", { name: "发指令" })).toBeVisible();
    });
    expect(within(detail).queryByRole("button", { name: "开始执行" })).not.toBeInTheDocument();
  });
});

describe("ClusterPage 创建任务弹窗", () => {
  it("「新建任务」打开弹窗；节点未选齐时提示第几步缺失并禁用提交", async () => {
    const user = userEvent.setup();
    renderWithQuery(fixturedBridge());

    expect(
      screen.queryByRole("dialog", { name: "新建编排任务" }),
    ).not.toBeInTheDocument();

    const dialog = await openCreateDialog(user);
    expect(within(dialog).getByRole("button", { name: "创建并开始" })).toBeDisabled();

    await user.type(within(dialog).getByLabelText("任务名称"), "未选Agent");
    await user.type(within(dialog).getByLabelText("任务描述"), "未选 Agent 的任务");
    await user.selectOptions(
      within(dialog).getByLabelText("工作流模板"),
      "template-standard",
    );
    await user.selectOptions(
      within(dialog).getByLabelText("工作目录"),
      "D:/Project/agent-notify",
    );

    expect(
      await within(dialog).findByText(
        "第 1、2、3 步未选择 Agent：全部选好后才能创建。",
      ),
    ).toBeVisible();
    expect(within(dialog).getByRole("button", { name: "创建并开始" })).toBeDisabled();
    expect(within(dialog).getByRole("button", { name: "仅创建" })).toBeDisabled();
  });

  it("任务名称必填（8 字以内）：未填名称时不能提交", async () => {
    const user = userEvent.setup();
    renderWithQuery(fixturedBridge());

    const dialog = await openCreateDialog(user);
    const nameInput = within(dialog).getByLabelText("任务名称");
    expect(nameInput).toHaveAttribute("maxlength", "8");
    expect(within(dialog).getByRole("button", { name: "仅创建" })).toBeDisabled();

    await user.type(within(dialog).getByLabelText("任务描述"), "只填描述");
    await user.selectOptions(
      within(dialog).getByLabelText("工作流模板"),
      "template-quickfix",
    );
    await user.selectOptions(
      within(dialog).getByLabelText("工作目录"),
      "D:/Project/agent-notify",
    );
    await within(dialog).findAllByRole("option", { name: /Alpha Agent/ });
    await user.selectOptions(
      within(dialog).getByLabelText("第 1 步 Agent"),
      "alpha",
    );
    await user.selectOptions(
      within(dialog).getByLabelText("第 2 步 Agent"),
      "alpha",
    );
    expect(within(dialog).getByRole("button", { name: "仅创建" })).toBeDisabled();

    await user.type(nameInput, "短名");
    expect(within(dialog).getByRole("button", { name: "仅创建" })).toBeEnabled();
  });

  it("「仅创建」提交全量节点配置（创建即锁定），任务出现在列表并展开", async () => {
    const user = userEvent.setup();
    const bridge = fixturedBridge();
    renderWithQuery(bridge);

    const dialog = await fillCreateDialog(user, {
      goal: "把登录流程加入重试机制",
    });
    await user.click(within(dialog).getByRole("button", { name: "仅创建" }));

    await waitFor(() => {
      expect(bridge.calls("create_orc_task")).toHaveLength(1);
    });
    expect(bridge.calls("create_orc_task")[0]?.payload).toEqual({
      goal: "把登录流程加入重试机制",
      name: "测试任务",
      notifyMode: "final_only",
      templateId: "template-standard",
      workingDir: "D:/Project/agent-notify",
      steps: [
        { order: 1, agent: "alpha", model: null },
        { order: 2, agent: "alpha", model: null },
        { order: 3, agent: "alpha", model: null },
      ],
    });
    // 仅创建不派活；新任务展开为「待开始」
    expect(bridge.calls("start_orc_task")).toHaveLength(0);
    expect(
      screen.queryByRole("dialog", { name: "新建编排任务" }),
    ).not.toBeInTheDocument();
    const detail = await screen.findByRole("article", { name: "任务详情" });
    expect(within(detail).getByRole("region", { name: "工作流" })).toBeVisible();
    expect(
      within(detail).getByRole("button", { name: "开始执行" }),
    ).toBeVisible();
  });

  it("「创建并开始」创建成功后立即派活第 1 步", async () => {
    const user = userEvent.setup();
    const bridge = fixturedBridge();
    renderWithQuery(bridge);

    const dialog = await fillCreateDialog(user, { goal: "一键开工的任务" });
    await user.click(within(dialog).getByRole("button", { name: "创建并开始" }));

    await waitFor(() => {
      expect(bridge.calls("start_orc_task")).toHaveLength(1);
    });
    expect(bridge.calls("start_orc_task")[0]?.payload).toEqual({
      taskId: "orc-1",
    });
  });

  it("选择「手动输入」后用填写的目录创建", async () => {
    const user = userEvent.setup();
    const bridge = fixturedBridge();
    renderWithQuery(bridge);

    const dialog = await openCreateDialog(user);
    await user.type(within(dialog).getByLabelText("任务名称"), "手动目录");
    await user.type(within(dialog).getByLabelText("任务描述"), "手动目录任务");
    await user.selectOptions(
      within(dialog).getByLabelText("工作流模板"),
      "template-standard",
    );
    await user.selectOptions(
      within(dialog).getByLabelText("工作目录"),
      "__manual__",
    );
    const manual = await within(dialog).findByLabelText("工作目录（手动输入）");
    await user.type(manual, "D:/Project/manual-app");
    await within(dialog).findAllByRole("option", { name: /Alpha Agent/ });
    for (let order = 1; order <= 3; order++) {
      await user.selectOptions(
        within(dialog).getByLabelText(`第 ${order} 步 Agent`),
        "alpha",
      );
    }
    await user.click(within(dialog).getByRole("button", { name: "仅创建" }));

    await waitFor(() => {
      expect(bridge.calls("create_orc_task")).toHaveLength(1);
    });
    expect(bridge.calls("create_orc_task")[0]?.payload).toMatchObject({
      name: "手动目录",
      workingDir: "D:/Project/manual-app",
    });
  });

  it("默认通知节奏为 final_only，可切换为逐步流转", async () => {
    const user = userEvent.setup();
    const bridge = fixturedBridge();
    renderWithQuery(bridge);

    const dialog = await fillCreateDialog(user, { goal: "默认节奏任务" });
    await user.selectOptions(
      within(dialog).getByLabelText("通知节奏"),
      "verbose",
    );
    await user.click(within(dialog).getByRole("button", { name: "仅创建" }));

    await waitFor(() => {
      expect(bridge.calls("create_orc_task")).toHaveLength(1);
    });
    expect(bridge.calls("create_orc_task")[0]?.payload).toMatchObject({
      name: "测试任务",
      notifyMode: "verbose",
    });
  });

  it("OpenCode 节点从下拉选模型（显示名 + provider/model），切到其他 Agent 时清空", async () => {
    const user = userEvent.setup();
    const bridge = createMockHostBridge({
      agents: [
        agentFixture("opencode", { displayName: "OpenCode" }),
        agentFixture("alpha", { displayName: "Alpha Agent" }),
      ],
    });
    renderWithQuery(bridge);

    const dialog = await openCreateDialog(user);
    await user.type(within(dialog).getByLabelText("任务名称"), "带模型");
    await user.type(within(dialog).getByLabelText("任务描述"), "带模型的任务");
    await user.selectOptions(
      within(dialog).getByLabelText("工作流模板"),
      "template-quickfix",
    );
    await user.selectOptions(
      within(dialog).getByLabelText("工作目录"),
      "D:/Project/agent-notify",
    );
    await within(dialog).findAllByRole("option", { name: /OpenCode/ });

    await user.selectOptions(
      within(dialog).getByLabelText("第 1 步 Agent"),
      "opencode",
    );
    const model = within(dialog).getByLabelText("第 1 步 模型");
    expect(model).toBeEnabled();
    // 选项 = 显示名 + provider/model：按名字选就不会拼错格式
    expect(
      within(model).getByRole("option", {
        name: /Space Bunny Free（opencode-go\/space-bunny-free）/,
      }),
    ).toBeInTheDocument();
    await user.selectOptions(model, "opencode-go/space-bunny-free");

    await user.selectOptions(
      within(dialog).getByLabelText("第 2 步 Agent"),
      "alpha",
    );
    // 非 OpenCode 不支持指定模型：输入框禁用并清空已填内容
    expect(within(dialog).getByLabelText("第 2 步 模型")).toBeDisabled();
    expect(within(dialog).getByText("该 Agent 暂不支持指定模型")).toBeVisible();

    await user.click(within(dialog).getByRole("button", { name: "仅创建" }));
    await waitFor(() => {
      expect(bridge.calls("create_orc_task")).toHaveLength(1);
    });
    expect(bridge.calls("create_orc_task")[0]?.payload).toMatchObject({
      name: "带模型",
      templateId: "template-quickfix",
      steps: [
        { order: 1, agent: "opencode", model: "opencode-go/space-bunny-free" },
        { order: 2, agent: "alpha", model: null },
      ],
    });
  });

  it("模型列表读取失败：区块顶部明确报错并退回手动输入", async () => {
    const user = userEvent.setup();
    const bridge = createMockHostBridge({
      agents: [agentFixture("opencode", { displayName: "OpenCode" })],
      errors: {
        list_opencode_models: {
          code: "opencode_models_unavailable",
          message:
            "读取 OpenCode 模型列表失败：连接 OpenCode 服务失败（请确认 OpenCode 桌面端已打开）",
          retryable: true,
        },
      },
    });
    renderWithQuery(bridge);

    const dialog = await openCreateDialog(user);
    await user.type(within(dialog).getByLabelText("任务名称"), "模型失败");
    await user.type(within(dialog).getByLabelText("任务描述"), "模型读不到也要能建");
    await user.selectOptions(
      within(dialog).getByLabelText("工作流模板"),
      "template-quickfix",
    );
    await within(dialog).findAllByRole("option", { name: /OpenCode/ });
    await user.selectOptions(
      within(dialog).getByLabelText("第 1 步 Agent"),
      "opencode",
    );

    const alert = await within(dialog).findByRole("alert");
    expect(alert).toHaveTextContent("无法读取 OpenCode 模型列表");
    expect(alert).toHaveTextContent(/请确认 OpenCode 桌面端已打开/);
    // 退回手动输入（不是死路）
    const manual = within(dialog).getByLabelText("第 1 步 模型");
    expect(manual).toBeEnabled();
    await user.type(manual, "opencode-go/space-bunny-free");

    await user.selectOptions(
      within(dialog).getByLabelText("第 2 步 Agent"),
      "opencode",
    );
    await user.selectOptions(
      within(dialog).getByLabelText("工作目录"),
      "D:/Project/agent-notify",
    );
    // 手动输入兜底也参与校验/提交
    await user.click(within(dialog).getByRole("button", { name: "仅创建" }));
    await waitFor(() => {
      expect(bridge.calls("create_orc_task")).toHaveLength(1);
    });
    expect(bridge.calls("create_orc_task")[0]?.payload).toMatchObject({
      name: "模型失败",
      steps: [
        { order: 1, agent: "opencode", model: "opencode-go/space-bunny-free" },
        { order: 2, agent: "opencode", model: null },
      ],
    });
  });

  it("模型列表读取失败后点击「重新读取」会重试请求", async () => {
    const user = userEvent.setup();
    const bridge = createMockHostBridge({
      agents: [agentFixture("opencode", { displayName: "OpenCode" })],
      errors: {
        list_opencode_models: {
          code: "opencode_models_unavailable",
          message: "读取 OpenCode 模型列表失败：连接 OpenCode 服务失败",
          retryable: true,
        },
      },
    });
    renderWithQuery(bridge);

    const dialog = await openCreateDialog(user);
    await user.selectOptions(
      within(dialog).getByLabelText("工作流模板"),
      "template-quickfix",
    );
    await within(dialog).findAllByRole("option", { name: /OpenCode/ });
    await user.selectOptions(
      within(dialog).getByLabelText("第 1 步 Agent"),
      "opencode",
    );
    await within(dialog).findByRole("alert");

    await user.click(within(dialog).getByRole("button", { name: "重新读取" }));
    await waitFor(() => {
      expect(bridge.calls("list_opencode_models")).toHaveLength(2);
    });
  });

  it("模板已有默认配置时预填节点，只需再填目标与目录", async () => {
    const user = userEvent.setup();
    const bridge = createMockHostBridge({
      agents: [
        agentFixture("opencode", { displayName: "OpenCode" }),
        agentFixture("codex", { displayName: "Codex" }),
      ],
      orcTemplates: orcTemplatesFixture().map((template) =>
        template.id === "template-standard"
          ? configuredTemplateFixture()
          : template,
      ),
    });
    renderWithQuery(bridge);

    const dialog = await openCreateDialog(user);
    const submit = within(dialog).getByRole("button", { name: "创建并开始" });
    await user.type(within(dialog).getByLabelText("任务名称"), "继承默认");
    await user.type(within(dialog).getByLabelText("任务描述"), "继承默认配置");
    await user.selectOptions(
      within(dialog).getByLabelText("工作流模板"),
      "template-standard",
    );
    expect(within(dialog).getByLabelText("第 1 步 Agent")).toHaveValue(
      "opencode",
    );
    await user.selectOptions(
      within(dialog).getByLabelText("工作目录"),
      "D:/Project/agent-notify",
    );
    expect(submit).toBeEnabled();

    await user.click(submit);
    await waitFor(() => {
      expect(bridge.calls("create_orc_task")).toHaveLength(1);
    });
    expect(bridge.calls("create_orc_task")[0]?.payload).toMatchObject({
      name: "继承默认",
      steps: [
        { order: 1, agent: "opencode", model: null },
        { order: 2, agent: "opencode", model: "anthropic/claude-sonnet-4-5" },
        { order: 3, agent: "codex", model: null },
      ],
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

    const dialog = await openCreateDialog(user);
    expect(await within(dialog).findByRole("alert")).toHaveTextContent(
      "无法读取 OpenCode 项目",
    );
    expect(
      within(dialog).getByText(/未找到 OpenCode 项目数据库/),
    ).toBeVisible();
    // 自动退回手动输入，且校验仍要求填写目录
    expect(
      within(dialog).getByLabelText("工作目录（手动输入）"),
    ).toBeVisible();
    expect(within(dialog).getByRole("button", { name: "创建并开始" })).toBeDisabled();

    await user.click(within(dialog).getByRole("button", { name: "重新读取" }));
    await waitFor(() => {
      expect(bridge.calls("list_opencode_projects")).toHaveLength(2);
    });
  });

  it("未读取到项目时提示手动输入（不静默使用空下拉）", async () => {
    const user = userEvent.setup();
    const bridge = createMockHostBridge({ opencodeProjects: [] });
    renderWithQuery(bridge);

    const dialog = await openCreateDialog(user);
    expect(
      await within(dialog).findByText(
        "未读取到 OpenCode 项目，请手动输入工作目录。",
      ),
    ).toBeVisible();
    expect(
      within(dialog).getByLabelText("工作目录（手动输入）"),
    ).toBeVisible();
  });

  it("模板读取失败：禁用模板选择并禁止提交，可重试", async () => {
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

    const dialog = await openCreateDialog(user);
    expect(await within(dialog).findByRole("alert")).toHaveTextContent(
      "无法读取工作流模板",
    );
    expect(within(dialog).getByText("模板列表暂时不可用。")).toBeVisible();
    expect(within(dialog).getByLabelText("工作流模板")).toBeDisabled();
    expect(within(dialog).getByRole("button", { name: "创建并开始" })).toBeDisabled();

    await user.click(within(dialog).getByRole("button", { name: "重新读取" }));
    await waitFor(() => {
      expect(bridge.calls("list_orc_templates")).toHaveLength(2);
    });
  });

  it("创建失败：弹窗内展示后端中文错误并保留已输入内容", async () => {
    const user = userEvent.setup();
    const bridge = createMockHostBridge({
      agents: twoAgents,
      errors: {
        create_orc_task: {
          code: "orc_working_dir_invalid",
          message: "工作目录不存在：D:/Project/gone",
          retryable: false,
        },
      },
    });
    renderWithQuery(bridge);

    const dialog = await fillCreateDialog(user, {
      goal: "会失败的任务",
      manualDir: "D:/Project/gone",
    });
    await user.click(within(dialog).getByRole("button", { name: "创建并开始" }));

    expect(await within(dialog).findByRole("alert")).toHaveTextContent(
      "工作目录不存在：D:/Project/gone",
    );
    expect(
      screen.getByRole("dialog", { name: "新建编排任务" }),
    ).toBeVisible();
    expect(within(dialog).getByLabelText("任务描述")).toHaveValue("会失败的任务");
    expect(bridge.calls("start_orc_task")).toHaveLength(0);
  });

  it("Escape 关闭弹窗且不创建任务", async () => {
    const user = userEvent.setup();
    const bridge = fixturedBridge();
    renderWithQuery(bridge);

    const dialog = await openCreateDialog(user);
    await user.type(within(dialog).getByLabelText("任务名称"), "半途放弃");
    await user.type(within(dialog).getByLabelText("任务描述"), "半途放弃");
    await user.keyboard("{Escape}");

    expect(
      screen.queryByRole("dialog", { name: "新建编排任务" }),
    ).not.toBeInTheDocument();
    expect(bridge.calls("create_orc_task")).toHaveLength(0);
  });
});

describe("ClusterPage 工作目录数据源", () => {
  it("工作目录下拉按最近活跃倒序展示「名称（目录）」，无名称只显示目录", async () => {
    const user = userEvent.setup();
    renderWithQuery(fixturedBridge());

    const dialog = await openCreateDialog(user);
    await within(dialog).findByRole("option", { name: /agent-notify/ });
    const select = within(dialog).getByLabelText("工作目录");
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

  it("已配置项目样本按最近活跃倒序（fixture 契约）", () => {
    const projects = opencodeProjectsFixture();
    expect(projects[0]?.name).toBe("agent-notify");
    expect(projects[1]?.name).toBeNull();
  });
});

describe("ClusterPage 编辑与删除", () => {
  it("编辑待开始任务：名称/描述/通知节奏可改并提交 update_orc_task", async () => {
    const user = userEvent.setup();
    const bridge = fixturedBridge([pendingTask]);
    renderWithQuery(bridge);

    await taskTable();
    await user.click(screen.getByRole("button", { name: "编辑任务 贪吃蛇" }));
    const dialog = await screen.findByRole("dialog", { name: "编辑任务" });
    const nameInput = within(dialog).getByLabelText("任务名称");
    await user.clear(nameInput);
    await user.type(nameInput, "改名任务");
    const goalInput = within(dialog).getByLabelText("任务描述");
    await user.clear(goalInput);
    await user.type(goalInput, "新的描述");
    await user.selectOptions(within(dialog).getByLabelText("通知节奏"), "verbose");
    await user.click(within(dialog).getByRole("button", { name: "保存修改" }));

    await waitFor(() => {
      expect(bridge.calls("update_orc_task")).toHaveLength(1);
    });
    expect(bridge.calls("update_orc_task")[0]?.payload).toEqual({
      taskId: "task-pending",
      name: "改名任务",
      goal: "新的描述",
      notifyMode: "verbose",
    });
    expect(
      screen.queryByRole("dialog", { name: "编辑任务" }),
    ).not.toBeInTheDocument();
    // 列表刷新后显示新名称
    const list = await taskTable();
    expect(within(list).getByRole("button", { name: "改名任务" })).toBeVisible();
  });

  it("编辑已开始任务：描述禁用且不提交 goal（只改名称）", async () => {
    const user = userEvent.setup();
    const bridge = fixturedBridge([workingTask]);
    renderWithQuery(bridge);

    await taskTable();
    await user.click(screen.getByRole("button", { name: "编辑任务 登录重试" }));
    const dialog = await screen.findByRole("dialog", { name: "编辑任务" });
    expect(within(dialog).getByLabelText("任务描述")).toBeDisabled();
    expect(within(dialog).getByText(/描述不可再改/)).toBeVisible();
    const nameInput = within(dialog).getByLabelText("任务名称");
    await user.clear(nameInput);
    await user.type(nameInput, "新短名");
    await user.click(within(dialog).getByRole("button", { name: "保存修改" }));

    await waitFor(() => {
      expect(bridge.calls("update_orc_task")).toHaveLength(1);
    });
    expect(bridge.calls("update_orc_task")[0]?.payload).toEqual({
      taskId: "task-working",
      name: "新短名",
      goal: null,
      notifyMode: "final_only",
    });
  });

  it("删除任务：确认后调用 delete_orc_task 并收起详情", async () => {
    const user = userEvent.setup();
    const bridge = fixturedBridge([workingTask, blockedTask]);
    renderWithQuery(bridge);

    await expandTask(user, "登录重试");
    await user.click(screen.getByRole("button", { name: "删除任务 登录重试" }));
    const confirm = await screen.findByRole("alertdialog");
    expect(confirm).toHaveTextContent("删除任务「登录重试」");
    await user.click(within(confirm).getByRole("button", { name: "确认删除" }));

    await waitFor(() => {
      expect(bridge.calls("delete_orc_task")).toHaveLength(1);
    });
    expect(bridge.calls("delete_orc_task")[0]?.payload).toEqual({
      taskId: "task-working",
    });
    expect(screen.queryByRole("alertdialog")).not.toBeInTheDocument();
    await waitFor(() => {
      expect(
        screen.queryByRole("button", { name: "登录重试" }),
      ).not.toBeInTheDocument();
    });
    expect(
      screen.queryByRole("article", { name: "任务详情" }),
    ).not.toBeInTheDocument();
  });

  it("删除确认可取消：不调用删除", async () => {
    const user = userEvent.setup();
    const bridge = fixturedBridge([workingTask]);
    renderWithQuery(bridge);

    await taskTable();
    await user.click(screen.getByRole("button", { name: "删除任务 登录重试" }));
    const confirm = await screen.findByRole("alertdialog");
    await user.click(within(confirm).getByRole("button", { name: "取消" }));
    expect(screen.queryByRole("alertdialog")).not.toBeInTheDocument();
    expect(bridge.calls("delete_orc_task")).toHaveLength(0);
  });

  it("编辑失败：在弹窗内展示后端中文错误并保留输入", async () => {
    const user = userEvent.setup();
    const bridge = createMockHostBridge({
      orcTasks: [pendingTask],
      errors: {
        update_orc_task: {
          code: "orc_task_already_started",
          message: "任务已开始，描述不可修改（可修改名称）",
          retryable: false,
        },
      },
    });
    renderWithQuery(bridge);

    await taskTable();
    await user.click(screen.getByRole("button", { name: "编辑任务 贪吃蛇" }));
    const dialog = await screen.findByRole("dialog", { name: "编辑任务" });
    const nameInput = within(dialog).getByLabelText("任务名称");
    await user.clear(nameInput);
    await user.type(nameInput, "改名");
    await user.click(within(dialog).getByRole("button", { name: "保存修改" }));

    expect(await within(dialog).findByRole("alert")).toHaveTextContent(
      "任务已开始，描述不可修改",
    );
    expect(within(dialog).getByLabelText("任务名称")).toHaveValue("改名");
  });
});
