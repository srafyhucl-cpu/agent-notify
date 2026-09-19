import { useQuery } from "@tanstack/react-query";

import type { HostBridge } from "../bridge";
import { DIAGNOSTICS_STALE_TIME_MS } from "./queryClient";
import { queryKeys } from "./queryKeys";

export function useDiagnostics(bridge: HostBridge) {
  return useQuery({
    queryKey: queryKeys.diagnostics(),
    queryFn: () => bridge.invoke("get_diagnostics", {}),
    staleTime: DIAGNOSTICS_STALE_TIME_MS,
  });
}
