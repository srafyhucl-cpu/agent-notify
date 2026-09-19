import { useQuery } from "@tanstack/react-query";

import type { HostBridge } from "../bridge";
import {
  SNAPSHOT_HIDDEN_REFETCH_INTERVAL_MS,
  SNAPSHOT_STALE_TIME_MS,
  SNAPSHOT_VISIBLE_REFETCH_INTERVAL_MS,
  windowAwareRefetchInterval,
} from "./queryClient";
import { queryKeys } from "./queryKeys";

export function useSnapshot(bridge: HostBridge) {
  return useQuery({
    queryKey: queryKeys.snapshot(),
    queryFn: () => bridge.invoke("get_snapshot", {}),
    staleTime: SNAPSHOT_STALE_TIME_MS,
    refetchInterval: windowAwareRefetchInterval(
      SNAPSHOT_VISIBLE_REFETCH_INTERVAL_MS,
      SNAPSHOT_HIDDEN_REFETCH_INTERVAL_MS,
    ),
  });
}
