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
  { path: "/overview", label: "总览", icon: LayoutDashboard },
  { path: "/agents", label: "Agents", icon: Bot },
  { path: "/channels", label: "Channels", icon: RadioTower },
  { path: "/history", label: "History", icon: History },
  { path: "/diagnostics", label: "Diagnostics", icon: Stethoscope },
  { path: "/settings", label: "Settings", icon: Settings },
] as const;
