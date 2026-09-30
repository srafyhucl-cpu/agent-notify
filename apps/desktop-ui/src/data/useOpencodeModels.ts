import { useQuery } from "@tanstack/react-query";

import type { HostBridge } from "../bridge";
import { queryKeys } from "./queryKeys";

/** 模型列表的取数策略：默认挂载即取（弹窗/设置页）；任务详情懒加载（B4）。 */
export interface UseOpencodeModelsOptions {
  /**
   * 是否立即请求（默认 true）。任务详情传 false：详情展开不请求，
   * 只有用户真正打开模型下拉时才置为 true，避免 OpenCode 未运行时在详情里堆错误。
   */
  enabled?: boolean;
}

/**
 * OpenCode 可用模型（模型下拉数据源）：只读本地服务，缓存键 `opencodeModels`（跨页面复用）；
 * 读取失败由调用方明确报错并退回手动输入（不猜、不静默），重试由用户显式触发。
 */
export function useOpencodeModels(
  bridge: HostBridge,
  options: UseOpencodeModelsOptions = {},
) {
  return useQuery({
    queryKey: queryKeys.opencodeModels(),
    queryFn: () => bridge.invoke("list_opencode_models", {}),
    enabled: options.enabled ?? true,
  });
}
