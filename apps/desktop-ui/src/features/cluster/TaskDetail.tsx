import { Pencil } from "lucide-react";

import type { OrcMessageKindDto, OrcTaskDto } from "../../bridge/types";
import { StatusBadge } from "../../components/patterns";
import {
  ORC_NOTIFY_MODE_LABELS,
  ORC_TERMINAL_STATES,
  formatOrcCreatedAt,
  orcNodeStatesOfTask,
  orcStepActions,
  orcStepEditable,
  orcTaskStateLabel,
  orcTaskStateTone,
  shortBlockReason,
} from "./labels";
import { OrcNodeChain } from "./OrcNodeChain";
import { RoundTimeline } from "./RoundTimeline";

export interface TaskDetailProps {
  task: OrcTaskDto;
  /** 任一操作进行中：统一禁用操作按钮，防止重复提交。 */
  busy: boolean;
  onStart: () => void;
  onAdvance: (kind: OrcMessageKindDto) => void;
  onRecover: () => void;
  /** 继续迭代（本轮结束后开始新一轮）：打开继续迭代弹窗。 */
  onContinue: () => void;
  /** 修改某一步的模型/思考强度（§12.4）：打开节点编辑弹窗。 */
  onEditStep: (order: number) => void;
}

/**
 * 任务详情（展开行内容）：任务描述 / 工作流（重点，推进操作跟随当前节点）/ 任务信息；
 * 任务名称只在列表行展示（详情里不重复标题）；失败原因在「任务阻塞」块里给出。
 */
export function TaskDetail({
  task,
  busy,
  onStart,
  onAdvance,
  onRecover,
  onContinue,
  onEditStep,
}: TaskDetailProps) {
  const blocked = task.blockedStep !== null;
  const terminal = ORC_TERMINAL_STATES.has(task.state);
  const stepEditable = orcStepEditable(task);
  const pendingStart = !task.started && !terminal;
  const stepLabel = blocked
    ? `阻塞在第 ${task.blockedStep ?? task.currentStep} 步`
    : task.finalizing
      ? `第 ${task.currentStep} 步（汇总中）`
      : `第 ${task.currentStep} 步`;
  const nodeStates = orcNodeStatesOfTask(task);
  const createdAt = formatOrcCreatedAt(task.createdAt, "full");
  const stepActions = orcStepActions(task);

  /**
   * 当前节点卡内的推进操作（跟随节点状态显示/隐藏）：
   * 待开始 → 开始执行；汇总中 → 只给说明；其余见 [`orcStepActions`]。
   */
  const renderStepActions = () => {
    if (blocked || terminal) {
      return null;
    }
    if (task.finalizing) {
      return (
        <p className="orc-node-actions-note">
          项目经理正在汇总，等待最终汇报；汇总完成后任务自动结束。
        </p>
      );
    }
    if (pendingStart) {
      return (
        <div className="orc-node-actions">
          <button
            className="button"
            type="button"
            title="开始执行：派活第 1 步（节点配置在创建时已锁定）"
            disabled={busy}
            onClick={onStart}
          >
            {busy ? "启动中…" : "开始执行"}
          </button>
        </div>
      );
    }
    if (stepActions.length === 0) {
      return null;
    }
    return (
      <div className="orc-node-actions">
        {stepActions.map((action) => (
          <button
            key={action.kind}
            type="button"
            title={action.hint}
            className={action.primary ? "button" : "button button-secondary"}
            disabled={busy}
            onClick={() => onAdvance(action.kind)}
          >
            {busy && action.primary ? "发送中…" : action.label}
          </button>
        ))}
      </div>
    );
  };

  return (
    <article className="cluster-task-detail" aria-label="任务详情">
      <section className="cluster-detail-pane" aria-label="任务描述">
        <h4 className="cluster-detail-pane-title">任务描述</h4>
        <p className="cluster-detail-goal">{task.goal}</p>
        <RoundTimeline
          goal={task.goal}
          currentRound={task.round}
          currentInput={task.roundInput}
          records={task.roundHistory}
          roundFinished={terminal}
        />
      </section>

      <section className="cluster-detail-pane" aria-label="工作流">
        <h4 className="cluster-detail-pane-title">工作流</h4>
        <OrcNodeChain
          label="工作流节点"
          steps={task.workflow.steps.map((step) => ({
            order: step.order,
            role: step.role,
            agent: step.agentHint,
            model: step.model,
            variant: step.variant,
            humanGate: step.humanGate,
          }))}
          nodeStates={nodeStates}
          renderDetails={(step) => (
            <>
              {step.order === task.currentStep ? renderStepActions() : null}
              {stepEditable ? (
                <div className="orc-node-edit">
                  <button
                    className="orc-node-edit-button"
                    type="button"
                    title={`修改第 ${step.order} 步的模型与思考强度`}
                    aria-label={`修改第 ${step.order} 步模型与思考强度`}
                    disabled={busy}
                    onClick={() => onEditStep(step.order)}
                  >
                    <Pencil aria-hidden="true" size={12} />
                    修改
                  </button>
                </div>
              ) : null}
            </>
          )}
        />
        {terminal ? (
          <div className="cluster-actions">
            <p className="cluster-actions-note">
              本轮已结束。想继续改就点「继续迭代」：回到第 1 步由项目经理重新规划，
              再走一遍实施与复核（可附上本轮要求或复核意见）。
            </p>
            <div className="cluster-actions-buttons">
              <button
                className="button"
                type="button"
                title="继续迭代：回到第 1 步由项目经理重新规划，再走一遍实施与复核（可附本轮要求）"
                disabled={busy}
                onClick={onContinue}
              >
                继续迭代（第 {task.round + 1} 轮）
              </button>
            </div>
          </div>
        ) : null}
      </section>

      <section className="cluster-detail-pane" aria-label="任务信息">
        <h4 className="cluster-detail-pane-title">任务信息</h4>
        <dl className="cluster-task-facts cluster-task-facts--inline">
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
            <dd>
              {ORC_NOTIFY_MODE_LABELS[task.notifyMode] ?? task.notifyMode}
            </dd>
          </div>
          <div className="cluster-task-fact">
            <dt>轮次</dt>
            <dd>第 {task.round} 轮</dd>
          </div>
          {createdAt ? (
            <div className="cluster-task-fact">
              <dt>创建于</dt>
              <dd>{createdAt}</dd>
            </div>
          ) : null}
          <div className="cluster-task-fact">
            <dt>工作目录</dt>
            <dd className="cluster-task-fact-dir">
              {task.workingDir ?? "跟随宿主当前项目"}
            </dd>
          </div>
        </dl>
      </section>

      {blocked ? (
        <div className="cluster-blocked-card" role="alert">
          <strong className="cluster-blocked-title">任务阻塞</strong>
          <p className="cluster-blocked-reason">
            {shortBlockReason(
              task.blockReason ??
                "任务执行失败：请打开对应会话查看原因后点「重新发起」",
            )}
          </p>
          <button
            className="button button-secondary"
            type="button"
            title="重新发起：清除阻塞，从被卡住的那一步重新派活"
            disabled={busy}
            onClick={onRecover}
          >
            {busy ? "处理中…" : "重新发起"}
          </button>
        </div>
      ) : null}
    </article>
  );
}
