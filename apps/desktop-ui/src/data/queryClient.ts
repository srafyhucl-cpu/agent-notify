import {
  QueryClient,
  keepPreviousData,
} from "@tanstack/react-query";

import type { CommandError } from "../bridge/types";

export const SNAPSHOT_STALE_TIME_MS = 5_000;
// 历史页仍可在显式刷新或失效后重取，避免页面停留期间频繁访问 SQLite。
export const HISTORY_STALE_TIME_MS = 30_000;
// 诊断只由页面挂载、手动刷新或诊断动作后的失效触发，不做后台轮询。
export const DIAGNOSTICS_STALE_TIME_MS = Number.POSITIVE_INFINITY;

export const SNAPSHOT_VISIBLE_REFETCH_INTERVAL_MS = 30_000;
export const SNAPSHOT_HIDDEN_REFETCH_INTERVAL_MS = 120_000;

const CACHE_MAX_AGE_MS = 5 * 60_000;
const MAX_DEFAULT_QUERY_RETRIES = 2;

export function isCommandError(error: unknown): error is CommandError {
  if (typeof error !== "object" || error === null) {
    return false;
  }

  return (
    "code" in error &&
    typeof error.code === "string" &&
    "message" in error &&
    typeof error.message === "string" &&
    "retryable" in error &&
    typeof error.retryable === "boolean"
  );
}

export function windowAwareRefetchInterval(
  visibleIntervalMs: number,
  hiddenIntervalMs: number,
) {
  return () =>
    typeof document !== "undefined" && document.visibilityState === "hidden"
      ? hiddenIntervalMs
      : visibleIntervalMs;
}

export function createQueryClient(): QueryClient {
  return new QueryClient({
    defaultOptions: {
      queries: {
        gcTime: CACHE_MAX_AGE_MS,
        placeholderData: keepPreviousData,
        refetchOnWindowFocus: true,
        // HostBridge 的业务错误原样暴露，只有网络等非业务错误允许默认重试。
        retry: (failureCount: number, error: unknown) =>
          failureCount < MAX_DEFAULT_QUERY_RETRIES && !isCommandError(error),
        staleTime: 0,
      },
      mutations: {
        retry: false,
      },
    },
  });
}
