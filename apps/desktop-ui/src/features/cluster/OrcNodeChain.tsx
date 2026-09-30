import { useEffect, useState, type ReactNode } from "react";

import type { OpencodeModelDto } from "../../bridge/types";
import {
  ORC_MODEL_CAPABLE_AGENT,
  ORC_NODE_STATE_LABELS,
  ORC_PROJECT_MANAGER_LABEL,
  ORC_ROLE_LABELS,
  orcStepAgentLabel,
  orcStepModelIntensityLabel,
  type OrcNodeState,
} from "./labels";
import { modelKeyOf } from "./OrcModelSelect";

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

/** 节点卡行内编辑模型/思考强度（§12.4）：任务结束前直接改，选择即保存。 */
export interface OrcNodeModelEditing {
  /** OpenCode 可用模型（null = 列表未取到：保留当前值，仅能清空为「不指定」）。 */
  models: OpencodeModelDto[] | null;
  /** 保存进行中：禁用行内下拉，避免并发写入。 */
  saving: boolean;
  /** 保存某步模型/强度：空串 = 清除（回到 Agent 默认模型 / 模型默认强度）。 */
  onSave: (order: number, model: string | null, variant: string | null) => void;
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
  /** 第二行右侧内容（任务详情 = 当前节点的推进操作）；缺省/返回 null 时留空占位。 */
  renderMetaActions?: (step: OrcNodeChainStep) => ReactNode;
  /** 模型/强度行内编辑（任务未结束时由详情页提供；仅对 OpenCode 节点生效）。 */
  modelEditing?: OrcNodeModelEditing;
  /** 布局：list = 竖向链（详情/设置）；grid = 栅格（创建弹窗，避免纵向过长）。 */
  layout?: "list" | "grid";
  className?: string;
}

/** 行内模型/强度编辑（§12.4）：选择即保存；保留草稿，保存期间下拉不回跳闪旧值。 */
function OrcNodeInlineModelFields({
  order,
  model,
  variant,
  editing,
}: {
  order: number;
  model: string | null;
  variant: string | null;
  editing: OrcNodeModelEditing;
}) {
  const savedModel = model?.trim() ?? "";
  const savedVariant = variant?.trim() ?? "";
  const [draftModel, setDraftModel] = useState(savedModel);
  const [draftVariant, setDraftVariant] = useState(savedVariant);

  // 保存完成/失败后与任务快照对齐；保存中保留草稿。
  useEffect(() => {
    if (!editing.saving) {
      setDraftModel(savedModel);
      setDraftVariant(savedVariant);
    }
  }, [editing.saving, savedModel, savedVariant]);

  const models = editing.models ?? [];
  const modelKnown = models.some(
    (candidate) => modelKeyOf(candidate) === draftModel,
  );
  const selectedModel = models.find(
    (candidate) => modelKeyOf(candidate) === draftModel,
  );
  // 强度候选 = 所选模型的 variants（原样英文 id，不翻译）+ 已保存但不在列表里的值（不静默丢弃）。
  const variantOptions = [...(selectedModel?.variants ?? [])];
  if (draftVariant && !variantOptions.includes(draftVariant)) {
    variantOptions.push(draftVariant);
  }

  return (
    <>
      <div className="orc-node-meta-item">
        <dt>模型</dt>
        <dd>
          <select
            className="orc-node-inline-select"
            aria-label={`第 ${order} 步 模型`}
            title={draftModel || "不指定（Agent 默认）"}
            value={draftModel}
            disabled={editing.saving}
            onChange={(event) => {
              const next = event.currentTarget.value;
              setDraftModel(next);
              // 换模型（含清空）时强度回到默认：旧强度不一定属于新模型。
              setDraftVariant("");
              editing.onSave(order, next, "");
            }}
          >
            <option value="">不指定（Agent 默认）</option>
            {draftModel !== "" && !modelKnown ? (
              <option value={draftModel}>{draftModel}</option>
            ) : null}
            {models.map((candidate) => {
              const key = modelKeyOf(candidate);
              return (
                <option value={key} key={key}>
                  {candidate.name}
                </option>
              );
            })}
          </select>
        </dd>
      </div>
      <div className="orc-node-meta-item">
        <dt>强度</dt>
        <dd>
          <select
            className="orc-node-inline-select"
            aria-label={`第 ${order} 步 思考强度`}
            title={draftVariant || "default"}
            value={draftVariant}
            disabled={editing.saving || draftModel === ""}
            onChange={(event) => {
              const next = event.currentTarget.value;
              setDraftVariant(next);
              editing.onSave(order, draftModel, next);
            }}
          >
            <option value="">default</option>
            {variantOptions.map((id) => (
              <option value={id} key={id}>
                {id}
              </option>
            ))}
          </select>
        </dd>
      </div>
    </>
  );
}

/**
 * 共用节点链（§7）：首节点标注「项目经理」；语义由颜色 + 简短中文标签承载，不用符号徽标。
 * 节点卡固定两行（摘要 + 操作），层层等高；模型/强度在任务未结束时于第二行内联编辑（§12.4）。
 * 动效只做注意力引导（当前脉冲 / 推进流光 / 完成收束 / 失败红光），去掉动效不丢信息；
 * `prefers-reduced-motion` 下全部退化为静态高亮（见 cluster.css）。
 */
export function OrcNodeChain({
  label,
  steps,
  nodeStates,
  showMeta = true,
  renderDetails,
  renderMetaActions,
  modelEditing,
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
        const agentLabel = orcStepAgentLabel(step.agent);
        const intensity = orcStepModelIntensityLabel(step.model, step.variant);
        // v1 仅 OpenCode 支持指定模型（§3）：其它 Agent / 未配置的节点保持只读。
        const stepModelEditing =
          (step.agent ?? "").trim() === ORC_MODEL_CAPABLE_AGENT
            ? modelEditing
            : undefined;
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
                <div className="orc-node-meta-row">
                  <dl className="orc-node-meta">
                    <div className="orc-node-meta-item">
                      <dt>Agent</dt>
                      <dd className="orc-node-meta-text" title={agentLabel}>
                        {agentLabel}
                      </dd>
                    </div>
                    {stepModelEditing ? (
                      <OrcNodeInlineModelFields
                        order={step.order}
                        model={step.model}
                        variant={step.variant ?? null}
                        editing={stepModelEditing}
                      />
                    ) : (
                      <div className="orc-node-meta-item">
                        <dt>模型</dt>
                        <dd className="orc-node-meta-text" title={intensity}>
                          {intensity}
                        </dd>
                      </div>
                    )}
                  </dl>
                  <div className="orc-node-meta-actions">
                    {renderMetaActions ? renderMetaActions(step) : null}
                  </div>
                </div>
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
