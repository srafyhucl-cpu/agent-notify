import { useQuery } from "@tanstack/react-query";

import type { HostBridge } from "../bridge";
import { queryKeys } from "./queryKeys";

/**
 * 当前编排工作流（节点列表）：供创建任务前预览「每步做什么、派给谁」。
 * 工作流由设置决定、不常变：不轮询，页面挂载时取一次，保存设置后由对应 mutation 失效。
 */
export function useOrcWorkflow(bridge: HostBridge) {
  return useQuery({
    queryKey: queryKeys.orcWorkflow(),
    queryFn: () => bridge.invoke("get_current_orc_workflow", {}),
  });
}