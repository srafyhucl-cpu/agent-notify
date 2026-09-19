import { Outlet } from "react-router-dom";

import type { HostBridge } from "../bridge";
import { AppNav } from "../components/AppNav";
import { RuntimeStatusBar } from "../components/RuntimeStatusBar";

export interface AppShellProps {
  bridge: HostBridge;
}

export function AppShell({ bridge }: AppShellProps) {
  return (
    <div className="app-shell">
      <AppNav />
      <RuntimeStatusBar bridge={bridge} />
      <main className="app-main" id="main-content" aria-label="AgentNotify 工作台">
        <Outlet />
      </main>
    </div>
  );
}
