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

/**
 * 节点模型/强度只读的终态（§12.4）：仅已完成/已取消/已拒绝不可改；
 * 阻塞（failed）可改后重新发起（与后端 `update_task_step` 校验一致）。
 */
export const ORC_STEP_EDIT_LOCKED_STATES: ReadonlySet<OrcTaskStateDto> = new Set([
  "completed",
  "canceled",
  "rejected",
]);

/** 该任务的节点模型/强度当前是否可改（任务结束前可改，§12.4）。 */
export function orcStepEditable(task: Pick<OrcTaskDto, "state">): boolean {
  return !ORC_STEP_EDIT_LOCKED_STATES.has(task.state);
}

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

/**
 * 思考强度（模型 variant）展示：**原样展示 id（不翻译）**，与服务端/模型侧取值保持一致；
 * 未知取值同样原样（服务端可能新增强度，不猜测含义）。
 */
export function orcVariantLabel(variant: string | null | undefined): string | null {
  const value = variant?.trim();
  return value ? value : null;
}

/**
 * 节点模型 + 思考强度展示（§12.4 详情节点卡）：如「模型 xxx · 强度 高」；
 * 模型为空沿用「未指定」；未指定强度时不追加。
 */
export function orcStepModelIntensityLabel(
  model: string | null | undefined,
  variant?: string | null,
): string {
  const base = orcStepModelLabel(model);
  const intensity = orcVariantLabel(variant);
  return intensity ? `${base} · 强度 ${intensity}` : base;
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
 * 阻塞原因的展示清洗：去掉任务 ID / 内部实现细节，把常见英文错误翻译成
 * 用户看得懂、知道怎么解决的一句话（新任务的 reason 已是干净文案，这里兜底旧数据）。
 */
export function shortBlockReason(reason: string): string {
  const text = reason.trim();
  const lower = text.toLowerCase();
  if (
    lower.includes("insufficient account funds") ||
    lower.includes("insufficient funds") ||
    lower.includes("quota")
  ) {
    return "模型服务余额不足：请充值，或给该节点换一个模型后点「重新发起」";
  }
  if (
    lower.includes("model unavailable") ||
    lower.includes("model not found") ||
    lower.includes("unknown model")
  ) {
    return "所选模型不可用：请给该节点换一个模型后点「重新发起」";
  }
  return text
    .replace(/任务\s+[0-9a-fA-F-]{36}\s*/g, "")
    .replace(/[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}/g, "")
    .replace(/（任务已阻塞，不会自动重推）/g, "")
    .replace(/派活给 Agent\s+(\S+)\s+失败：/g, "无法唤醒 $1：")
    .replace(/\bStep\s+(\d+)\s*/g, "第 $1 步")
    .replace(/\s{2,}/g, " ")
    .trim();
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
 * 当前节点的推进操作（§4.2 消息总线 kind 的子集，随节点状态显示/隐藏）：
 * 精简为最多 2 个按钮，集成在工作流当前节点卡内（不再单开「操作」区块）。
 */
export interface OrcStepAction {
  kind: OrcMessageKindDto;
  label: string;
  /** 悬浮一句话说明（鼠标停在按钮上就能看懂是干嘛的） */
  hint: string;
  primary?: boolean;
}

/**
 * 当前节点可用操作：
 * - 阻塞 / 终态 / 汇总中：无（阻塞用「重新发起」、终态用「继续迭代」、汇总等首节点汇报）；
 * - 待开始：无（「开始执行」由详情组件单独渲染在当前节点卡内）；
 * - 等待人工确认（input_required）：确认完成（主）+ 发指令（要求返工）；
 * - 干活中：发指令（主）+ 汇报（Agent 没自动汇报时人工补报，推进到下一步）。
 */
export function orcStepActions(
  task: Pick<OrcTaskDto, "state" | "started" | "finalizing" | "blockedStep">,
): OrcStepAction[] {
  if (
    ORC_TERMINAL_STATES.has(task.state) ||
    task.blockedStep !== null ||
    task.finalizing ||
    !task.started
  ) {
    return [];
  }
  if (task.state === "input_required") {
    return [
      {
        kind: "confirm",
        label: "确认完成",
        hint: "通过这一步的人工确认门：确认后推进到下一步",
        primary: true,
      },
      {
        kind: "instruction",
        label: "发指令",
        hint: "要求返工或补充：这一步回到干活状态继续改",
      },
    ];
  }
  return [
    {
      kind: "instruction",
      label: "发指令",
      hint: "给这一步的 Agent 发一条指令：补充要求或纠正方向，它回到本步继续干活",
      primary: true,
    },
    {
      kind: "report",
      label: "汇报",
      hint: "以人工身份替这一步提交产出汇报：推进到下一步（Agent 没自动汇报时用）",
    },
  ];
}

/**
 * 任务创建时间展示（本地时区）：列表用短格式 MM-DD HH:mm，详情用完整格式 YYYY-MM-DD HH:mm。
 * 旧任务无创建时间或值非法时返回 null（不显示、不猜）。
 */
export function formatOrcCreatedAt(
  value: string | null | undefined,
  style: "short" | "full" = "short",
): string | null {
  if (!value) {
    return null;
  }
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) {
    return null;
  }
  const pad = (part: number) => String(part).padStart(2, "0");
  const day = `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())}`;
  const time = `${pad(date.getHours())}:${pad(date.getMinutes())}`;
  return style === "full" ? `${day} ${time}` : `${day.slice(5)} ${time}`;
}