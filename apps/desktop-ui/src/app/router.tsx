import {
  QueryClientContext,
  QueryClientProvider,
} from "@tanstack/react-query";
import { useContext, useState } from "react";
import { Navigate, Route, Routes } from "react-router-dom";

import type { HostBridge } from "../bridge";
import { createQueryClient } from "../data/queryClient";
import { useHostEvent } from "../data/useHostEvent";
import { AgentsPage } from "../features/agents/AgentsPage";
import { ChannelsPage } from "../features/channels/ChannelsPage";
import { DiagnosticsPage } from "../features/diagnostics/DiagnosticsPage";
import { HistoryPage } from "../features/history/HistoryPage";
import { OverviewPage } from "../features/overview/OverviewPage";
import { SettingsPage } from "../features/settings/SettingsPage";
import { AppShell } from "./AppShell";
import "../styles/task9.css";
import "../styles/task7.css";

export interface AppRouterProps {
  bridge: HostBridge;
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
        <Route path="/history" element={<HistoryPage bridge={bridge} />} />
        <Route
          path="/diagnostics"
          element={<DiagnosticsPage bridge={bridge} />}
        />
        <Route path="/settings" element={<SettingsPage bridge={bridge} />} />
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
