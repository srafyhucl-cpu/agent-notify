import { StrictMode } from "react";
import { createRoot } from "react-dom/client";

import { App } from "../../src/app/App";
import { createMockHostBridge } from "../../src/bridge";
import type { MockHostBridgeOptions } from "../../src/bridge";
import "../../src/styles/tokens.css";
import "../../src/styles/reset.css";
import "../../src/styles/layout.css";

/**
 * 仅用于 Playwright 的浏览器挂载点：
 * 复用真实 App、真实路由和真实 mock HostBridge，避免为测试改动生产入口。
 */
const rootElement = document.getElementById("root");
if (!rootElement) {
  throw new Error("找不到 UI 测试挂载点");
}

const scenario: MockHostBridgeOptions =
  window.__AGENT_NOTIFY_HARNESS__?.scenario ?? {};
const bridge = createMockHostBridge(scenario);
window.__AGENT_NOTIFY_BRIDGE__ = bridge;

createRoot(rootElement).render(
  <StrictMode>
    <App bridge={bridge} />
  </StrictMode>,
);
