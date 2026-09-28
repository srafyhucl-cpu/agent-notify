import { useQuery } from "@tanstack/react-query";

import type { HostBridge } from "../bridge";
import { queryKeys } from "./queryKeys";

/**
 * OpenCode 已知项目（工作目录下拉数据源，§3.1）：按最近活跃倒序。
 * 读取失败由创建表单明确报错并退回手动输入（不静默）；不轮询，重试由用户显式触发。
 */
export function useOpencodeProjects(bridge: HostBridge) {
  return useQuery({
    queryKey: queryKeys.opencodeProjects(),
    queryFn: () => bridge.invoke("list_opencode_projects", {}),
  });
}
