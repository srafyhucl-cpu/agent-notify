import { Navigate, Route, Routes } from "react-router-dom";

import type { HostBridge } from "../bridge";
import { EmptyState } from "../components/EmptyState";
import { AppShell } from "./AppShell";
import { navigationItems } from "./navigation";
import type { NavigationItem } from "./navigation";

export interface AppRouterProps {
  bridge: HostBridge;
}

function WorkbenchPage({ item }: { item: NavigationItem }) {
  const titleId = `page-title-${item.path.slice(1)}`;

  return (
    <section className="workbench-page" aria-labelledby={titleId}>
      <header className="workbench-page-header">
        <h1 className="workbench-page-title" id={titleId}>
          {item.label}
        </h1>
      </header>
      <div className="workbench-page-content">
        <EmptyState description="当前没有可显示的内容。" />
      </div>
    </section>
  );
}

export function AppRouter({ bridge }: AppRouterProps) {
  return (
    <Routes>
      <Route element={<AppShell bridge={bridge} />}>
        <Route index element={<Navigate replace to="/overview" />} />
        {navigationItems.map((item) => (
          <Route
            key={item.path}
            path={item.path}
            element={<WorkbenchPage item={item} />}
          />
        ))}
        <Route path="*" element={<Navigate replace to="/overview" />} />
      </Route>
    </Routes>
  );
}
