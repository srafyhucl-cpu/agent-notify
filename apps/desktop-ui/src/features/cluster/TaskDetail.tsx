import type { OrcMessageKindDto, OrcTaskDto } from "../../bridge/types";
import { SectionCard, StatusBadge } from "../../components/patterns";
import {
  ORC_MESSAGE_KIND_ACTIONS,
  ORC_NOTIFY_MODE_LABELS,
  ORC_TASK_STATE_LABELS,
  ORC_TASK_STATE_TONES,
  ORC_TERMINAL_STATES,
} from "./labels";

export interface TaskDetailProps {
  task: OrcTaskDto | null;
  /** 任一操作进行中：统一禁用操作按钮，防止重复提交。 */
  busy: boolean;
  onAdvance: (kind: OrcMessageKindDto) => void;
  onRecover: () => void;
}

/** 空详情占位：未选中任务时的引导文案。 */
function EmptyDetail() {
  return (
    <div
      className="cluster-task-detail cluster-task-detail--empty"
      aria-label="任务详情"
    >
      <h2>未选择任务</h2>
      <p>从左侧选择一个任务，查看进度并下达指令。</p>
    </div>
  );
}

/**
 * 任务详情：状态卡 + Step 进度 + 发指令（advance，§4.2 消息 kind）；
 * 阻塞任务展示原因与「重新发起」（recover_blocked），不提供推进。
 */
export function TaskDetail({
  task,
  busy,
  onAdvance,
  onRecover,
}: TaskDetailProps) {
  if (!task) {
    return <EmptyDetail />;
  }

  const blocked = task.blockedStep !== null;
  const terminal = ORC_TERMINAL_STATES.has(task.state);
  const stepLabel = blocked
    ? `阻塞在第 ${task.blockedStep ?? task.currentStep} 步`
    : `第 ${task.currentStep} 步`;

  return (
    <article className="cluster-task-detail" aria-label="任务详情">
      <SectionCard title={task.goal}>
        <dl className="cluster-task-facts">
          <div className="cluster-task-fact">
            <dt>状态</dt>
            <dd>
              <StatusBadge tone={ORC_TASK_STATE_TONES[task.state]}>
                {ORC_TASK_STATE_LABELS[task.state]}
              </StatusBadge>
            </dd>
          </div>
          <div className="cluster-task-fact">
            <dt>当前步骤</dt>
            <dd>{stepLabel}</dd>
          </div>
          <div className="cluster-task-fact">
            <dt>通知节奏</dt>
            <dd>
              {ORC_NOTIFY_MODE_LABELS[task.notifyMode] ?? task.notifyMode}
            </dd>
          </div>
          <div className="cluster-task-fact">
            <dt>任务 ID</dt>
            <dd className="cluster-task-fact-id">{task.id}</dd>
          </div>
        </dl>

        {blocked ? (
          <div className="cluster-blocked-card" role="alert">
            <strong className="cluster-blocked-title">任务阻塞</strong>
            <p className="cluster-blocked-reason">
              {task.blockReason ?? "投递失败，需要人工处理后重新发起。"}
            </p>
            <button
              className="button button-secondary"
              type="button"
              disabled={busy}
              onClick={onRecover}
            >
              {busy ? "处理中…" : "重新发起"}
            </button>
          </div>
        ) : terminal ? (
          <p className="cluster-terminal-note">任务已结束，无待办操作。</p>
        ) : (
          <div className="cluster-actions">
            <p className="cluster-actions-label">推进任务</p>
            <div className="cluster-actions-buttons">
              {ORC_MESSAGE_KIND_ACTIONS.map((action) => (
                <button
                  key={action.kind}
                  type="button"
                  className={action.primary ? "button" : "button button-secondary"}
                  disabled={busy}
                  onClick={() => onAdvance(action.kind)}
                >
                  {busy && action.primary ? "发送中…" : action.label}
                </button>
              ))}
            </div>
          </div>
        )}
      </SectionCard>
    </article>
  );
}