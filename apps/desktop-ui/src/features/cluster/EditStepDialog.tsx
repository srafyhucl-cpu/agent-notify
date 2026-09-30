import { X } from "lucide-react";
import { useEffect, useRef, useState, type MouseEvent } from "react";

import type { HostBridge } from "../../bridge";
import type { OrcTaskDto } from "../../bridge/types";
import { InlineError } from "../../components/InlineError";
import { toUserError } from "../../data/errors";
import { useOpencodeModels } from "../../data/useOpencodeModels";
import { ORC_MODEL_CAPABLE_AGENT, orcVariantLabel } from "./labels";
import { OrcModelSelect, modelKeyOf } from "./OrcModelSelect";

/** 节点编辑提交内容（§12.4）：空串 = 清除（回到 Agent 默认模型 / 模型默认强度）。 */
export interface EditStepInput {
  model: string;
  variant: string;
}

export interface EditStepDialogProps {
  bridge: HostBridge;
  task: OrcTaskDto;
  /** 目标步骤序号（必须存在于任务工作流）。 */
  order: number;
  /** 保存请求进行中：禁用提交并展示按钮内加载态。 */
  pending: boolean;
  /** 保存失败原因（已转成用户可读文案）；对话框内展示，不关闭。 */
  error: unknown;
  onSubmit: (input: EditStepInput) => Promise<boolean>;
  onClose: () => void;
}

/**
 * 修改节点模型与思考强度（§12.4）：任务结束前可改，只影响该步的步骤快照。
 * 沿用创建弹窗的表单语言：模型下拉 = OpenCode 模型列表 +「不指定（Agent 默认）」；
 * 思考强度下拉 = 所选模型的 variants +「默认」。
 */
export function EditStepDialog({
  bridge,
  task,
  order,
  pending,
  error,
  onSubmit,
  onClose,
}: EditStepDialogProps) {
  const step =
    task.workflow.steps.find((candidate) => candidate.order === order) ?? null;
  const modelsQuery = useOpencodeModels(bridge);
  const modelsError = modelsQuery.error
    ? toUserError(modelsQuery.error).message
    : null;
  const [modelValue, setModelValue] = useState(step?.model?.trim() ?? "");
  const [variantValue, setVariantValue] = useState(step?.variant?.trim() ?? "");
  const closeButtonRef = useRef<HTMLButtonElement>(null);
  const userError = error ? toUserError(error) : null;

  useEffect(() => {
    closeButtonRef.current?.focus();
    const handleKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        onClose();
      }
    };
    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, [onClose]);

  // 节点被并发删除等异常数据下不渲染半残表单（正常入口保证节点存在）。
  if (!step) {
    return null;
  }

  // 与后端同规则：v1 仅 OpenCode 支持指定模型。
  const modelEditable = step.agentHint?.trim() === ORC_MODEL_CAPABLE_AGENT;
  const trimmedModel = modelValue.trim();
  const trimmedVariant = variantValue.trim();
  const originalModel = step.model?.trim() ?? "";
  const originalVariant = step.variant?.trim() ?? "";

  // 所选模型的真实强度列表（去重）；已保存但不在列表里的强度保留一项，避免静默丢弃。
  const variantOptions: string[] = [];
  const selectedModel = modelsQuery.data?.find(
    (model) => modelKeyOf(model) === trimmedModel,
  );
  for (const id of selectedModel?.variants ?? []) {
    if (id && !variantOptions.includes(id)) {
      variantOptions.push(id);
    }
  }
  if (trimmedVariant && !variantOptions.includes(trimmedVariant)) {
    variantOptions.push(trimmedVariant);
  }

  const variantDisabled = pending || !modelEditable || trimmedModel === "";
  const canSave =
    !pending &&
    modelEditable &&
    (trimmedModel !== originalModel || trimmedVariant !== originalVariant);
  const hint = !modelEditable
    ? `该 Agent（${step.agentHint?.trim() || "未配置"}）暂不支持指定模型`
    : trimmedModel === ""
      ? "不指定模型时，思考强度会一并清除（回到 Agent 默认）"
      : "思考强度是所选模型的 variant；选「默认」使用该模型默认强度。";

  const handleOverlayMouseDown = (event: MouseEvent<HTMLDivElement>) => {
    if (event.target === event.currentTarget) {
      onClose();
    }
  };

  const submit = async () => {
    if (!canSave) {
      return;
    }
    await onSubmit({ model: trimmedModel, variant: trimmedVariant });
  };

  return (
    <div
      className="dialog-overlay"
      role="presentation"
      onMouseDown={handleOverlayMouseDown}
    >
      <section
        className="cluster-create-dialog cluster-create-dialog--compact"
        role="dialog"
        aria-modal="true"
        aria-labelledby="cluster-edit-step-title"
        aria-busy={pending}
      >
        <header className="dialog-header">
          <div>
            <h2 id="cluster-edit-step-title">修改第 {order} 步</h2>
            <p className="dialog-subtitle">
              只改这一步的模型与思考强度，任务结束前随时可调；Agent
              与工作流结构不变。
            </p>
          </div>
          <button
            ref={closeButtonRef}
            className="icon-button"
            type="button"
            aria-label="关闭修改窗口"
            onClick={onClose}
          >
            <X aria-hidden="true" size={18} />
          </button>
        </header>

        <div className="cluster-dialog-body">
          {userError ? (
            <InlineError title={userError.title} message={userError.message} />
          ) : null}

          {modelsError ? (
            <InlineError
              title="无法读取 OpenCode 模型列表"
              message={modelsError}
              action={
                <button
                  className="button button-secondary"
                  type="button"
                  onClick={() => void modelsQuery.refetch()}
                >
                  重新读取
                </button>
              }
            />
          ) : null}

          <div className="cluster-create-fields">
            <label>
              <span>模型</span>
              <OrcModelSelect
                ariaLabel={`第 ${order} 步 模型`}
                value={modelValue}
                onChange={(next) => {
                  setModelValue(next);
                  // 换模型（含清空）时强度回到默认：旧强度不一定属于新模型。
                  setVariantValue("");
                }}
                editable={modelEditable}
                disabled={pending}
                models={modelsQuery.data ?? null}
                error={modelsError}
                emptyOptionLabel="不指定（Agent 默认）"
              />
            </label>

            <label>
              <span>思考强度</span>
              <select
                aria-label={`第 ${order} 步 思考强度`}
                value={variantValue}
                disabled={variantDisabled}
                onChange={(event) => setVariantValue(event.currentTarget.value)}
              >
                <option value="">默认</option>
                {variantOptions.map((id) => (
                  <option value={id} key={id}>
                    {orcVariantLabel(id) ?? id}
                  </option>
                ))}
              </select>
            </label>

            <p className="cluster-edit-step-hint">{hint}</p>
          </div>
        </div>

        <div className="cluster-dialog-actions">
          <button
            className="button button-secondary"
            type="button"
            disabled={pending}
            onClick={onClose}
          >
            取消
          </button>
          <button
            className="button"
            type="button"
            disabled={!canSave}
            onClick={() => void submit()}
          >
            {pending ? "保存中…" : "保存"}
          </button>
        </div>
      </section>
    </div>
  );
}
