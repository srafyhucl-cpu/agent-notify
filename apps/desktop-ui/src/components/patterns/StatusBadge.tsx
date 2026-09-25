import type { ReactNode } from "react";

/** 状态徽标语义色。 */
export type StatusBadgeTone =
  | "success"
  | "warning"
  | "danger"
  | "info"
  | "neutral";

/** 状态徽标属性：tone + 文字。 */
export interface StatusBadgeProps {
  /** 语义色：柔和底 + 语义色文字 */
  tone: StatusBadgeTone;
  /** 徽标文字（语义由「颜色 + 文字」承担，不靠颜色单载） */
  children: ReactNode;
  /** 呼吸微光开关；默认仅 warning/danger 开启，传 false 显式关闭 */
  breath?: boolean;
  /** 根元素附加类名（仅用于外层布局） */
  className?: string;
}

/** 默认开启呼吸微光的语义色（档位 1：仅 warning/danger）。 */
const DEFAULT_BREATH_TONES: ReadonlySet<StatusBadgeTone> = new Set([
  "warning",
  "danger",
]);

/**
 * 状态徽标：柔和底 + 语义色文字 + 胶囊形。
 * 呼吸微光档位 1 仅 warning/danger 默认开启（周期约 2.5s、幅度极低；
 * 暗色 = 辉光脉冲、亮色 = 极淡阴影脉冲，均以 currentColor 派生同源色）；
 * success/info/neutral 静止；prefers-reduced-motion 全关。动效仅作注意力引导，不承载信息。
 */
export function StatusBadge({
  tone,
  children,
  breath,
  className,
}: StatusBadgeProps) {
  const breathing = breath ?? DEFAULT_BREATH_TONES.has(tone);
  const classes = ["pattern-status-badge"];
  if (breathing) {
    classes.push("pattern-status-badge--breath");
  }
  if (className) {
    classes.push(className);
  }

  return (
    <span className={classes.join(" ")} data-tone={tone}>
      {children}
    </span>
  );
}
