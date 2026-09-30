import type { ReactNode } from "react";

import {
  ORC_NODE_STATE_LABELS,
  ORC_PROJECT_MANAGER_LABEL,
  ORC_ROLE_LABELS,
  orcStepAgentLabel,
  orcStepModelIntensityLabel,
  type OrcNodeState,
} from "./labels";

/** 节点链单步：用途/Agent/模型/人工确认门（任务详情、创建预览、设置页共用）。 */
export interface OrcNodeChainStep {
  order: number;
  role: string;
  agent: string | null;
  model: string | null;
  /** 思考强度（模型 variant，§12.4）；缺省/空 = 未指定（模型默认强度）。 */
  variant?: string | null;
  humanGate?: boolean;
}

export interface OrcNodeChainProps {
  /** 节点链的无障碍名（如「工作流节点」）。 */
  label: string;
  steps: OrcNodeChainStep[];
  /** 节点状态（任务详情）；缺省 = 预览/配置态，无进度语义。 */
  nodeStates?: Record<number, OrcNodeState>;
  /** 是否展示只读的 Agent/模型摘要行（默认展示；设置页由编辑控件承担，传 false）。 */
  showMeta?: boolean;
  /** 每步附加内容（设置页的节点编辑控件等）。 */
  renderDetails?: (step: OrcNodeChainStep) => ReactNode;
  /** 布局：list = 竖向链（详情/设置）；grid = 栅格（创建弹窗，避免纵向过长）。 */
  layout?: "list" | "grid";
  className?: string;
}

/**
 * 共用节点链（§7）：首节点标注「项目经理」；语义由颜色 + 简短中文标签承载，不用符号徽标。
 * 动效只做注意力引导（当前脉冲 / 推进流光 / 完成收束 / 失败红光），去掉动效不丢信息；
 * `prefers-reduced-motion` 下全部退化为静态高亮（见 cluster.css）。
 */
export function OrcNodeChain({
  label,
  steps,
  nodeStates,
  showMeta = true,
  renderDetails,
  layout = "list",
  className,
}: OrcNodeChainProps) {
  const classes = ["orc-node-chain"];
  if (nodeStates) {
    classes.push("orc-node-chain--live");
  }
  if (layout === "grid") {
    classes.push("orc-node-chain--grid");
  }
  if (className) {
    classes.push(className);
  }

  return (
    <ol className={classes.join(" ")} aria-label={label}>
      {steps.map((step) => {
        const state: OrcNodeState = nodeStates?.[step.order] ?? "idle";
        const stateLabel = ORC_NODE_STATE_LABELS[state];
        return (
          <li
            className="orc-node"
            data-state={state}
            data-order={step.order}
            key={step.order}
          >
            {step.order > 1 ? (
              <span className="orc-node-flow" aria-hidden="true" />
            ) : null}
            <div className="orc-node-card">
              <div className="orc-node-head">
                <span className="orc-node-order">第 {step.order} 步</span>
                <span className="orc-node-role">
                  {ORC_ROLE_LABELS[step.role] ?? step.role}
                </span>
                {step.order === 1 ? (
                  <span className="orc-node-manager">
                    {ORC_PROJECT_MANAGER_LABEL}
                  </span>
                ) : null}
                {stateLabel ? (
                  <span className="orc-node-state">{stateLabel}</span>
                ) : null}
                {step.humanGate ? (
                  <span className="orc-node-gate">需人工确认</span>
                ) : null}
              </div>

              {showMeta ? (
                <dl className="orc-node-meta">
                  <div className="orc-node-meta-item">
                    <dt>Agent</dt>
                    <dd>{orcStepAgentLabel(step.agent)}</dd>
                  </div>
                  <div className="orc-node-meta-item">
                    <dt>模型</dt>
                    <dd>{orcStepModelIntensityLabel(step.model, step.variant)}</dd>
                  </div>
                </dl>
              ) : null}

              {renderDetails ? (
                <div className="orc-node-details">{renderDetails(step)}</div>
              ) : null}
            </div>
          </li>
        );
      })}
    </ol>
  );
}
