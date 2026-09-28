import { useQuery } from "@tanstack/react-query";

import type { HostBridge } from "../bridge";
import { queryKeys } from "./queryKeys";

/**
 * OpenCode 可用模型（模型下拉数据源）：只读本地服务，页面挂载时取一次；
 * 读取失败由弹窗/设置页明确报错并退回手动输入（不猜、不静默），重试由用户显式触发。
 */
export function useOpencodeModels(bridge: HostBridge) {
  return useQuery({
    queryKey: queryKeys.opencodeModels(),
    queryFn: () => bridge.invoke("list_opencode_models", {}),
  });
}
