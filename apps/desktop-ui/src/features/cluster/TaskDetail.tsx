import type { OrcMessageKindDto, OrcTaskDto } from "../../bridge/types";
import { StatusBadge } from "../../components/patterns";
import {
  ORC_MESSAGE_KIND_ACTIONS,
  ORC_NOTIFY_MODE_LABELS,
  ORC_TERMINAL_STATES,
  orcNodeStatesOfTask,
  orcTaskStateLabel,
  orcTaskStateTone,
} from "./labels";
import { OrcNodeChain } from "./OrcNodeChain";

export interface TaskDetailProps {
  task: OrcTaskDto;
  /** 任一操作进行中：统一禁用操作按钮，防止重复提交。 */
  busy: boolean;
  onStart: () => void;
  onAdvance: (kind: OrcMessageKindDto) => void;
  onRecover: () => void;
}

/**
 * 任务详情（展开行内容）：**工作流节点链置顶**（当前节点动效 + 中文状态标签），
 * 其下是任务信息网格与操作区；阻塞任务展示原因与「重新发起」，不提供推进。
 */
export function TaskDetail({
  task,
  busy,
  onStart,
  onAdvance,
  onRecover,
}: TaskDetailProps) {
  const blocked = task.blockedStep !== null;
  const terminal = ORC_TERMINAL_STATES.has(task.state);
  const pendingStart = !task.started && !terminal;
  const stepLabel = blocked
    ? `阻塞在第 ${task.blockedStep ?? task.currentStep} 步`
    : task.finalizing
      ? `第 ${task.currentStep} 步（汇总中）`
      : `第 ${task.currentStep} 步`;
  const nodeStates = orcNodeStatesOfTask(task);

  return (
    <article className="cluster-task-detail" aria-label="任务详情">
      <div className="cluster-detail-head">
        <h3 className="cluster-detail-name">{task.name}</h3>
        <p className="cluster-detail-goal">{task.goal}</p>
      </div>

      <div className="cluster-workflow">
        <p className="cluster-workflow-title">工作流</p>
        <OrcNodeChain
          label="工作流节点"
          steps={task.workflow.steps.map((step) => ({
            order: step.order,
            role: step.role,
            agent: step.agentHint,
            model: step.model,
            humanGate: step.humanGate,
          }))}
          nodeStates={nodeStates}
        />
      </div>

      <dl className="cluster-task-facts">
        <div className="cluster-task-fact">
          <dt>状态</dt>
          <dd>
            <StatusBadge tone={orcTaskStateTone(task)}>
              {orcTaskStateLabel(task)}
            </StatusBadge>
          </dd>
        </div>
        <div className="cluster-task-fact">
          <dt>当前步骤</dt>
          <dd>{stepLabel}</dd>
        </div>
        <div className="cluster-task-fact">
          <dt>通知节奏</dt>
          <dd>{ORC_NOTIFY_MODE_LABELS[task.notifyMode] ?? task.notifyMode}</dd>
        </div>
        <div className="cluster-task-fact">
          <dt>工作目录</dt>
          <dd className="cluster-task-fact-dir">
            {task.workingDir ?? "跟随宿主当前项目"}
          </dd>
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
      ) : task.finalizing ? (
        <div className="cluster-actions cluster-actions--finalizing">
          <p className="cluster-actions-label">项目经理汇总中</p>
          <p className="cluster-actions-note">
            项目经理正在汇总，等待最终汇报；汇总完成后任务自动结束。
          </p>
        </div>
      ) : pendingStart ? (
        <div className="cluster-actions">
          <p className="cluster-actions-label">任务待开始</p>
          <p className="cluster-actions-note">
            节点在创建时已锁定（每步做什么、派给谁都在上方工作流里）；点「开始执行」派活第
            1 步，中途不想跑可先不开始。
          </p>
          <div className="cluster-actions-buttons">
            <button
              className="button"
              type="button"
              disabled={busy}
              onClick={onStart}
            >
              {busy ? "启动中…" : "开始执行"}
            </button>
          </div>
        </div>
      ) : (
        <div className="cluster-actions">
          <p className="cluster-actions-label">推进任务</p>
          <div className="cluster-actions-buttons">
            {ORC_MESSAGE_KIND_ACTIONS.map((action) => (
              <button
                key={action.kind}
                type="button"
                className={
                  action.primary ? "button" : "button button-secondary"
                }
                disabled={busy}
                onClick={() => onAdvance(action.kind)}
              >
                {busy && action.primary ? "发送中…" : action.label}
              </button>
            ))}
          </div>
        </div>
      )}
    </article>
  );
}
