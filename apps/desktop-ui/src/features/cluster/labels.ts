import type {
  OrcMessageKindDto,
  OrcTaskDto,
  OrcTaskStateDto,
} from "../../bridge/types";
import type { StatusBadgeTone } from "../../components/patterns";

/** 编排任务状态 → 中文文案（§8.3 A2A TaskState 映射的桌面呈现侧）。 */
export const ORC_TASK_STATE_LABELS: Record<OrcTaskStateDto, string> = {
  unspecified: "未指定",
  submitted: "已提交",
  working: "执行中",
  completed: "已完成",
  failed: "已失败",
  canceled: "已取消",
  input_required: "等待输入",
  rejected: "已拒绝",
  auth_required: "需要认证",
};

/** 编排任务状态 → 语义色（颜色只作辅助，文字承担语义）。 */
export const ORC_TASK_STATE_TONES: Record<OrcTaskStateDto, StatusBadgeTone> = {
  unspecified: "neutral",
  submitted: "info",
  working: "info",
  completed: "success",
  failed: "danger",
  canceled: "neutral",
  input_required: "warning",
  rejected: "neutral",
  auth_required: "warning",
};

/** 终态：任务已结束，不再提供推进/恢复操作。 */
export const ORC_TERMINAL_STATES: ReadonlySet<OrcTaskStateDto> = new Set([
  "completed",
  "canceled",
  "rejected",
  "failed",
]);

/** 通知节奏（§4.6）：列表/详情展示用文案。 */
export const ORC_NOTIFY_MODE_LABELS: Record<string, string> = {
  final_only: "只推最终汇报",
  verbose: "逐步流转",
};

/** 工作流节点角色 → 中文说明（§3.1.1：每步做什么，便于创建前预览）。 */
export const ORC_ROLE_LABELS: Record<string, string> = {
  orchestrator: "初步判断",
  planner: "规划整理",
  executor: "实施",
  reviewer: "复核汇总",
};

/** 首节点 = 项目经理：负责汇总各步产出并向人做最终汇报（§2/§4）。 */
export const ORC_PROJECT_MANAGER_LABEL = "项目经理";

/** 任务名称长度上限（字）：短名只用于集群列表与真实会话标题展示。 */
export const TASK_NAME_MAX_CHARS = 8;

/** v1 仅 OpenCode 支持指定模型（§3）；设置页与创建任务弹窗共用。 */
export const ORC_MODEL_CAPABLE_AGENT = "opencode";

/** 节点 Agent 显示（配置/预览）：未选择时明确提示「未配置」，不猜默认值。 */
export function orcStepAgentLabel(agent: string | null | undefined): string {
  const value = agent?.trim();
  return value ? value : "未配置";
}

/**
 * 节点模型显示（预览/详情）：空 = 未指定（由该 Agent 自己决定，OpenCode 用其当前默认模型）。
 * 不写「默认模型」这种含糊说法，避免让人以为我们替他选了模型。
 */
export function orcStepModelLabel(model: string | null | undefined): string {
  const value = model?.trim();
  return value ? value : "未指定";
}

/** 是否处于「已创建但还没开始」的阶段（待开始；终态任务不算）。 */
function isPendingStart(task: Pick<OrcTaskDto, "state" | "started">): boolean {
  return !task.started && !ORC_TERMINAL_STATES.has(task.state);
}

/**
 * 任务状态展示文案：未开始的任务单独呈现为「待开始」（底层状态是 Working，
 * 直接显示「执行中」会把还没派活的任务误读成在跑）。
 */
export function orcTaskStateLabel(
  task: Pick<OrcTaskDto, "state" | "started">,
): string {
  return isPendingStart(task)
    ? "待开始"
    : ORC_TASK_STATE_LABELS[task.state];
}

/** 任务状态展示语义色：与 orcTaskStateLabel 配套（待开始 = 中性灰）。 */
export function orcTaskStateTone(
  task: Pick<OrcTaskDto, "state" | "started">,
): StatusBadgeTone {
  return isPendingStart(task) ? "neutral" : ORC_TASK_STATE_TONES[task.state];
}

/**
 * 节点链状态（可视化 §7）：动效只做注意力引导，语义由「颜色 + 简短中文标签」承载。
 * `idle` = 预览/配置态（无进度语义），`pending` = 未执行。
 */
export type OrcNodeState =
  | "idle"
  | "pending"
  | "current"
  | "done"
  | "failed"
  | "finalizing";

/** 节点状态 → 中文标签；无进度语义的状态不显示标签。 */
export const ORC_NODE_STATE_LABELS: Record<OrcNodeState, string | null> = {
  idle: null,
  pending: null,
  current: "当前节点",
  done: "已完成",
  failed: "执行失败",
  finalizing: "项目经理汇总中",
};

/**
 * 任务 → 各节点状态（§7）：当前节点脉冲、完成收束、失败红光；
 * `finalizing` 时首节点回到脉冲态（等项目经理汇总），其余节点已完成。
 */
export function orcNodeStatesOfTask(
  task: OrcTaskDto,
): Record<number, OrcNodeState> {
  const states: Record<number, OrcNodeState> = {};
  const terminal = ORC_TERMINAL_STATES.has(task.state);

  for (const step of task.workflow.steps) {
    if (terminal && task.state === "completed") {
      states[step.order] = "done";
      continue;
    }
    if (task.blockedStep !== null && step.order === task.blockedStep) {
      states[step.order] = "failed";
      continue;
    }
    if (task.finalizing) {
      states[step.order] = step.order === 1 ? "finalizing" : "done";
      continue;
    }
    if (!terminal && step.order === task.currentStep) {
      states[step.order] = "current";
      continue;
    }
    states[step.order] = step.order < task.currentStep ? "done" : "pending";
  }

  return states;
}

/** 创建任务表单的通知节奏选项。 */
export const ORC_NOTIFY_MODE_OPTIONS: ReadonlyArray<{
  value: string;
  label: string;
}> = [
  { value: "final_only", label: "只推最终汇报（默认）" },
  { value: "verbose", label: "逐步流转" },
];

/**
 * 推进任务的指令类型（§4.2 消息总线 kind）。
 * `primary` 只给最常见的「发指令」，其余为次要操作；全部经 advance_orc_task 发送。
 */
export const ORC_MESSAGE_KIND_ACTIONS: ReadonlyArray<{
  kind: OrcMessageKindDto;
  label: string;
  primary?: boolean;
}> = [
  { kind: "instruction", label: "发指令", primary: true },
  { kind: "confirm", label: "确认完成" },
  { kind: "report", label: "汇报" },
  { kind: "question", label: "提问" },
  { kind: "info", label: "补充信息" },
];