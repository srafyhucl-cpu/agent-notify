import type { ReactNode } from "react";

/** 一决策一行属性：左侧「标签 + 可选说明」，右侧控件列。 */
export interface FieldRowProps {
  /** 左侧标签 */
  label: string;
  /** 标签下说明（可选） */
  description?: string;
  /** 右侧控件；建议控件自带可访问名（aria-label，或传入 controlId 由本组件关联） */
  control: ReactNode;
  /** 控件 id；提供时左侧标签渲染为 <label htmlFor>，保证可访问名关联 */
  controlId?: string;
}

/**
 * 一决策一行：设置/详情页的通用行。
 * 行间以 --hairline 分隔、组内 8px 节奏；控件列右对齐，勾选框不再飘到最右边缘。
 * 交互与状态由调用方提供的控件承担，本组件只负责容器与排版。
 */
export function FieldRow({
  label,
  description,
  control,
  controlId,
}: FieldRowProps) {
  return (
    <div className="pattern-field-row">
      <div className="pattern-field-row-text">
        {controlId ? (
          <label className="pattern-field-row-label" htmlFor={controlId}>
            {label}
          </label>
        ) : (
          <span className="pattern-field-row-label">{label}</span>
        )}
        {description ? (
          <p className="pattern-field-row-description">{description}</p>
        ) : null}
      </div>
      <div className="pattern-field-row-control">{control}</div>
    </div>
  );
}
