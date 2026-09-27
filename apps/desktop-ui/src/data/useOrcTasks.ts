import { useQuery } from "@tanstack/react-query";

import type { HostBridge } from "../bridge";
import {
  SNAPSHOT_HIDDEN_REFETCH_INTERVAL_MS,
  SNAPSHOT_STALE_TIME_MS,
  SNAPSHOT_VISIBLE_REFETCH_INTERVAL_MS,
  windowAwareRefetchInterval,
} from "./queryClient";
import { queryKeys } from "./queryKeys";

/** 编排任务列表：任务状态会被编排层推进/阻塞，沿用快照级轮询保持桌面端及时。 */
export function useOrcTasks(bridge: HostBridge) {
  return useQuery({
    queryKey: queryKeys.orcTasks(),
    queryFn: () => bridge.invoke("list_orc_tasks", {}),
    staleTime: SNAPSHOT_STALE_TIME_MS,
    refetchInterval: windowAwareRefetchInterval(
      SNAPSHOT_VISIBLE_REFETCH_INTERVAL_MS,
      SNAPSHOT_HIDDEN_REFETCH_INTERVAL_MS,
    ),
  });
}