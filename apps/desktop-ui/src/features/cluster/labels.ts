import type {
  OrcMessageKindDto,
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