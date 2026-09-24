import type { ReactNode } from "react";

/** KPI 语义色：只影响状态点与数字旁缀，不整卡变色。 */
export type KpiTone = "default" | "success" | "warning" | "danger";

/** KPI 锚点卡属性：大数字 + 小标签 + 可选状态点/旁缀。 */
export interface KpiCardProps {
  /** 大数字（视觉锚点），使用 --font-display */
  value: ReactNode;
  /** 小标签（说明数字含义），使用 --font-caption */
  label: string;
  /** 语义色；default 不显示状态点，也不给旁缀上色 */
  tone?: KpiTone;
  /** 数字旁缀（单位 / 趋势等），随 tone 取语义色 */
  suffix?: ReactNode;
}

/**
 * KPI 锚点卡：每屏的视觉锚点，一屏一锚点。
 * tone 只作用于状态点与数字旁缀，卡片表面始终用统一表面语义层，避免"整卡变色"抢层级。
 */
export function KpiCard({
  value,
  label,
  tone = "default",
  suffix,
}: KpiCardProps) {
  return (
    <div className="pattern-kpi-card" data-tone={tone}>
      <div className="pattern-kpi-card-value">
        <span className="pattern-kpi-card-number">{value}</span>
        {suffix ? (
          <span className="pattern-kpi-card-suffix">{suffix}</span>
        ) : null}
      </div>
      <div className="pattern-kpi-card-label">
        {tone !== "default" ? (
          <span className="pattern-kpi-card-dot" aria-hidden="true" />
        ) : null}
        <span>{label}</span>
      </div>
    </div>
  );
}
