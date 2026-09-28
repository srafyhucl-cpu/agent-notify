import { useQuery } from "@tanstack/react-query";

import type { HostBridge } from "../bridge";
import { queryKeys } from "./queryKeys";

/**
 * 固定工作流模板列表（三档内置 + 节点配置合并结果）：设置页节点配置与创建任务预览共用。
 * 模板定义由代码常量决定、不常变：不轮询，页面挂载时取一次，保存节点配置后由 mutation 失效。
 */
export function useOrcTemplates(bridge: HostBridge) {
  return useQuery({
    queryKey: queryKeys.orcTemplates(),
    queryFn: () => bridge.invoke("list_orc_templates", {}),
  });
}
