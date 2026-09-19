import { QueryClientProvider } from "@tanstack/react-query";
import { render } from "@testing-library/react";
import type { ReactElement, ReactNode } from "react";
import { MemoryRouter } from "react-router-dom";

import { createQueryClient } from "../data/queryClient";

export interface RenderWithBridgeOptions {
  /** 指定初始路由，默认使用内存路由的根路径。 */
  route?: string;
}

/**
 * 为组件测试提供独立的 QueryClient 与路由上下文。
 * 每次调用都会新建缓存，避免用例之间互相污染。
 */
export function renderWithBridge(
  ui: ReactElement,
  options: RenderWithBridgeOptions = {},
) {
  const queryClient = createQueryClient();
  const routed: ReactNode =
    options.route === undefined ? (
      ui
    ) : (
      <MemoryRouter initialEntries={[options.route]}>{ui}</MemoryRouter>
    );

  return render(
    <QueryClientProvider client={queryClient}>{routed}</QueryClientProvider>,
  );
}
