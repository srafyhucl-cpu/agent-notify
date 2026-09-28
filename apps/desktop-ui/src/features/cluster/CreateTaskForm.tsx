import { Plus } from "lucide-react";
import { useState, type FormEvent } from "react";

import type { OrcWorkflowDto } from "../../bridge/types";
import { SectionCard } from "../../components/patterns";
import { ORC_NOTIFY_MODE_OPTIONS, ORC_ROLE_LABELS, orcAgentLabel } from "./labels";

export interface CreateTaskFormProps {
  /** 创建请求进行中：禁用提交并展示按钮内加载态。 */
  pending: boolean;
  /** 当前工作流（节点预览）；尚未取到时为 null，表单仍可用。 */
  workflow: OrcWorkflowDto | null;
  /** 提交目标与通知节奏（final_only / verbose）；返回成功与否，成功才清空表单。 */
  onSubmit: (goal: string, notifyMode: string) => Promise<boolean>;
}

/**
 * 创建编排任务（P1，§4.6/§3.3）：目标 + 通知节奏 + 工作流节点预览。
 * 创建后任务为「待开始」：用户在详情里看清节点后点「开始执行」才会派活第 1 步。
 */
export function CreateTaskForm({ pending, workflow, onSubmit }: CreateTaskFormProps) {
  const [goal, setGoal] = useState("");
  const [notifyMode, setNotifyMode] = useState("final_only");
  const canSubmit = goal.trim().length > 0 && !pending;

  const handleSubmit = async (event: FormEvent) => {
    event.preventDefault();
    if (!canSubmit) {
      return;
    }
    const created = await onSubmit(goal.trim(), notifyMode);
    if (created) {
      setGoal("");
    }
  };

  return (
    <SectionCard
      title="创建任务"
      description="布置一个集群任务，编排层按工作流逐步唤醒对应 Agent；创建后先确认节点，再点「开始执行」派活。"
    >
      <form
        className="cluster-create-form"
        aria-label="创建集群任务"
        onSubmit={handleSubmit}
      >
        {workflow ? (
          <div className="cluster-workflow-preview" aria-label="工作流节点预览">
            <p className="cluster-workflow-preview-title">
              {workflow.name}（{workflow.steps.length} 步）
            </p>
            <ol className="cluster-workflow-step-list">
              {workflow.steps.map((step) => (
                <li className="cluster-workflow-step" key={step.order}>
                  <span className="cluster-workflow-step-order">
                    第 {step.order} 步
                  </span>
                  <span className="cluster-workflow-step-role">
                    {ORC_ROLE_LABELS[step.role] ?? step.role}
                  </span>
                  <span className="cluster-workflow-step-agent">
                    {orcAgentLabel(step.agentHint)}
                  </span>
                  {step.humanGate ? (
                    <span className="cluster-workflow-step-gate">需人工确认</span>
                  ) : null}
                </li>
              ))}
            </ol>
          </div>
        ) : null}

        <div className="cluster-create-fields">
          <label className="cluster-create-goal-field">
            <span>目标</span>
            <textarea
              aria-label="目标"
              className="cluster-create-goal"
              rows={2}
              placeholder="例如：把登录流程加入重试机制"
              value={goal}
              onChange={(event) => setGoal(event.currentTarget.value)}
            />
          </label>

          <label className="cluster-create-notify-field">
            <span>通知节奏</span>
            <select
              aria-label="通知节奏"
              value={notifyMode}
              onChange={(event) => setNotifyMode(event.currentTarget.value)}
            >
              {ORC_NOTIFY_MODE_OPTIONS.map((option) => (
                <option value={option.value} key={option.value}>
                  {option.label}
                </option>
              ))}
            </select>
          </label>
        </div>

        <div className="cluster-create-actions">
          <button className="button" type="submit" disabled={!canSubmit}>
            <Plus aria-hidden="true" size={16} />
            {pending ? "创建中…" : "创建任务"}
          </button>
        </div>
      </form>
    </SectionCard>
  );
}