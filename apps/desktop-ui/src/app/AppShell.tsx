import { useState } from "react";
import { Outlet } from "react-router-dom";

import type { HostBridge } from "../bridge";
import { AppNav } from "../components/AppNav";
import { RuntimeStatusBar } from "../components/RuntimeStatusBar";

export interface AppShellProps {
  bridge: HostBridge;
}

export function AppShell({ bridge }: AppShellProps) {
  const [collapsed, setCollapsed] = useState(() => {
    try {
      return localStorage.getItem("agentnotify.sidebar_collapsed") === "true";
    } catch {
      return false;
    }
  });

  const toggleCollapsed = () => {
    setCollapsed((prev) => {
      const next = !prev;
      try {
        localStorage.setItem("agentnotify.sidebar_collapsed", String(next));
      } catch {}
      return next;
    });
  };

  return (
    <div className={`app-shell ${collapsed ? "app-shell--collapsed" : ""}`}>
      <AppNav collapsed={collapsed} onToggleCollapse={toggleCollapsed} />
      <RuntimeStatusBar bridge={bridge} />
      <main className="app-main" id="main-content" aria-label="AgentNotify 工作台">
        <Outlet />
      </main>
    </div>
  );
}
