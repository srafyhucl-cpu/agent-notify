import type { MockHostBridge, MockHostBridgeOptions } from "../../src/bridge";

declare global {
  interface Window {
    __AGENT_NOTIFY_HARNESS__?: {
      scenario?: MockHostBridgeOptions;
    };
    __AGENT_NOTIFY_BRIDGE__?: MockHostBridge;
  }
}

export {};
