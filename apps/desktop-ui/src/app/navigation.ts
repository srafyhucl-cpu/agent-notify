import {
  Bot,
  History,
  LayoutDashboard,
  RadioTower,
  Settings,
  Stethoscope,
} from "lucide-react";
import type { LucideIcon } from "lucide-react";

export type AppRoutePath =
  | "/overview"
  | "/agents"
  | "/channels"
  | "/history"
  | "/diagnostics"
  | "/settings";

export interface NavigationItem {
  path: AppRoutePath;
  label: string;
  icon: LucideIcon;
}

export const navigationItems: readonly NavigationItem[] = [
  { path: "/channels", label: "渠道", icon: RadioTower },
  { path: "/agents", label: "Agent 管理", icon: Bot },
  { path: "/overview", label: "总览", icon: LayoutDashboard },
  { path: "/history", label: "历史", icon: History },
  { path: "/diagnostics", label: "诊断", icon: Stethoscope },
  { path: "/settings", label: "设置", icon: Settings },
] as const;
