// 范式组件统一出口（barrel）。
// 在此引入 patterns.css：页面一旦 import 本 barrel，样式即随模块图进入，
// 真实入口与 Playwright 测试 harness 都会自动取得，无需改动 main.tsx / harness。
import "../../styles/patterns.css";

export { EmptyFunnel } from "./EmptyFunnel";
export type { EmptyFunnelProps, EmptyFunnelStep } from "./EmptyFunnel";
export { FieldRow } from "./FieldRow";
export type { FieldRowProps } from "./FieldRow";
export { PageHeader } from "./PageHeader";
export type { PageHeaderProps } from "./PageHeader";
export { SectionCard } from "./SectionCard";
export type { SectionCardProps } from "./SectionCard";
export { StatusBadge } from "./StatusBadge";
export type { StatusBadgeProps, StatusBadgeTone } from "./StatusBadge";
