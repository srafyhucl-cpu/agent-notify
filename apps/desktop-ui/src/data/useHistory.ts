import { useInfiniteQuery, useQuery } from "@tanstack/react-query";

import type { HostBridge } from "../bridge";
import type { NotificationFilterPayload } from "../bridge/types";
import { HISTORY_STALE_TIME_MS } from "./queryClient";
import { queryKeys } from "./queryKeys";

export const HISTORY_PAGE_SIZE = 10_000;

export type HistoryFilters = Omit<NotificationFilterPayload, "cursor" | "limit">;

export function useHistory(bridge: HostBridge, filters: HistoryFilters) {
  const queryFilters: NotificationFilterPayload = {
    ...filters,
    cursor: null,
    limit: HISTORY_PAGE_SIZE,
  };

  return useInfiniteQuery({
    queryKey: queryKeys.notifications(queryFilters),
    queryFn: ({ pageParam }) =>
      bridge.invoke("list_notifications", {
        ...filters,
        cursor: pageParam,
        limit: HISTORY_PAGE_SIZE,
      }),
    initialPageParam: null as string | null,
    getNextPageParam: (lastPage) => lastPage.nextCursor,
    staleTime: HISTORY_STALE_TIME_MS,
  });
}

export function useNotificationDetail(
  bridge: HostBridge,
  notificationId: string | null,
) {
  return useQuery({
    queryKey: queryKeys.notificationDetail(notificationId ?? undefined),
    queryFn: () => {
      if (!notificationId) {
        throw new Error("缺少通知 ID");
      }
      return bridge.invoke("get_notification_detail", { notificationId });
    },
    enabled: notificationId !== null,
    staleTime: HISTORY_STALE_TIME_MS,
  });
}
