import {
  QueryClientContext,
  QueryClientProvider,
} from "@tanstack/react-query";
import { useContext, useState } from "react";
import { Navigate, Route, Routes } from "react-router-dom";

import type { HostBridge } from "../bridge";
import { EmptyState } from "../components/EmptyState";
import { createQueryClient } from "../data/queryClient";
import { useHostEvent } from "../data/useHostEvent";
import { AgentsPage } from "../features/agents/AgentsPage";
import { ChannelsPage } from "../features/channels/ChannelsPage";
import { OverviewPage } from "../features/overview/OverviewPage";
import { AppShell } from "./AppShell";
import { navigationItems } from "./navigation";
import type { NavigationItem } from "./navigation";
import "../styles/task7.css";

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

function RoutedApp({ bridge }: AppRouterProps) {
  useHostEvent(bridge);

  return (
    <Routes>
      <Route element={<AppShell bridge={bridge} />}>
        <Route index element={<Navigate replace to="/overview" />} />
        <Route path="/overview" element={<OverviewPage bridge={bridge} />} />
        <Route path="/agents" element={<AgentsPage bridge={bridge} />} />
        <Route path="/channels" element={<ChannelsPage bridge={bridge} />} />
        {navigationItems
          .filter(
            (item) =>
              item.path !== "/overview" &&
              item.path !== "/agents" &&
              item.path !== "/channels",
          )
          .map((item) => (
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

export function AppRouter({ bridge }: AppRouterProps) {
  const inheritedQueryClient = useContext(QueryClientContext);
  const [fallbackQueryClient] = useState(createQueryClient);
  const queryClient = inheritedQueryClient ?? fallbackQueryClient;

  return (
    <QueryClientProvider client={queryClient}>
      <RoutedApp bridge={bridge} />
    </QueryClientProvider>
  );
}
