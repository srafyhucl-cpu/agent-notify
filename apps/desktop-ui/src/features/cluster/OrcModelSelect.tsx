import { useState } from "react";

import type { OpencodeModelDto } from "../../bridge/types";

/** 下拉里的「手动输入」哨兵值（模型 id 不会与它同名）。 */
const MANUAL_MODEL = "__manual__";
/** 手动输入项的文案（与工作目录的「手动输入」同款交互）。 */
const MANUAL_MODEL_LABEL = "手动输入 provider/model";

/** 模型显示键：`provider/model`（与后端/插件契约一致）。 */
export function modelKeyOf(model: OpencodeModelDto): string {
  return `${model.providerId}/${model.modelId}`;
}

export interface OrcModelSelectProps {
  /** 无障碍名（如「第 1 步 模型」）；手动输入框为「…（手动输入）」。 */
  ariaLabel: string;
  /** 当前模型值（`provider/model`；空 = 未指定）。 */
  value: string;
  onChange: (value: string) => void;
  /** false = 该 Agent 不支持指定模型：禁用输入并保持原值展示。 */
  editable: boolean;
  /** 保存/提交进行中：整体禁用（不改变形态）。 */
  disabled?: boolean;
  /** 可用模型（null = 尚未取到）；有值时给出下拉，避免手填 provider/model 拼错。 */
  models: OpencodeModelDto[] | null;
  /** 读取失败原因（已转中文）；非空时退回手动输入（错误提示由外层区块统一展示）。 */
  error: string | null;
  /** 控件类名（设置页用 settings-control；弹窗用节点字段自有样式）。 */
  controlClassName?: string;
}

/**
 * 模型选择（设置页与创建任务弹窗共用）：
 * - OpenCode 模型下拉（显示名 + `provider/model`，值即契约格式）+「未指定」；
 * - 下拉提供「手动输入」（兜底：列表里没有 / 读取失败时仍可手填）；
 * - 已保存但不在列表里的值按手动输入展示，不丢配置。
 */
export function OrcModelSelect({
  ariaLabel,
  value,
  onChange,
  editable,
  disabled = false,
  models,
  error,
  controlClassName,
}: OrcModelSelectProps) {
  const [manualVisible, setManualVisible] = useState(false);
  const controlClass = controlClassName ? ` ${controlClassName}` : "";

  if (!editable) {
    return (
      <input
        className={`orc-model-control${controlClass}`}
        aria-label={ariaLabel}
        type="text"
        placeholder="provider/model"
        value={value}
        disabled
        onChange={() => undefined}
      />
    );
  }

  if (error) {
    // 读取失败：明确错误由外层区块统一展示一次；这里退回手动输入（不静默）。
    return (
      <input
        className={`orc-model-control${controlClass}`}
        aria-label={ariaLabel}
        type="text"
        placeholder="provider/model"
        value={value}
        disabled={disabled}
        onChange={(event) => onChange(event.currentTarget.value)}
      />
    );
  }

  const known = models?.some((model) => modelKeyOf(model) === value) ?? false;
  const manualMode = manualVisible || (value !== "" && !known);

  return (
    <div className="orc-model-fields">
      <select
        className={`orc-model-control${controlClass}`}
        aria-label={ariaLabel}
        value={manualMode ? MANUAL_MODEL : value}
        disabled={disabled}
        onChange={(event) => {
          const next = event.currentTarget.value;
          if (next === MANUAL_MODEL) {
            setManualVisible(true);
            return;
          }
          setManualVisible(false);
          onChange(next);
        }}
      >
        <option value="">未指定（由 OpenCode 用其当前默认模型）</option>
        {models === null ? (
          <option value="" disabled>
            正在读取模型列表…
          </option>
        ) : null}
        {(models ?? []).map((model) => {
          const key = modelKeyOf(model);
          return (
            <option value={key} key={key}>
              {model.name}（{key}）
            </option>
          );
        })}
        <option value={MANUAL_MODEL}>{MANUAL_MODEL_LABEL}</option>
      </select>

      {manualMode ? (
        <input
          className={`orc-model-control${controlClass}`}
          aria-label={`${ariaLabel}（手动输入）`}
          type="text"
          placeholder="provider/model"
          value={value}
          disabled={disabled}
          onChange={(event) => onChange(event.currentTarget.value)}
        />
      ) : null}
    </div>
  );
}
