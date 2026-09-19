import { useQuery } from "@tanstack/react-query";

import type { HostBridge } from "../bridge";
import { queryKeys } from "./queryKeys";

const SETTINGS_STALE_TIME_MS = 30_000;

export function useSettings(bridge: HostBridge) {
  return useQuery({
    queryKey: queryKeys.settings(),
    queryFn: () => bridge.invoke("get_settings", {}),
    staleTime: SETTINGS_STALE_TIME_MS,
  });
}
