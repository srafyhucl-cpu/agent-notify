import { describe, expect, it, vi } from "vitest";

import { createMockHostBridge } from "./mockHostBridge";

describe("HostBridge", () => {
  it("records invocations and returns stable snapshot data", async () => {
    const bridge = createMockHostBridge();
    const first = bridge.invoke("get_snapshot", {});
    const second = bridge.invoke("get_snapshot", {});

    await expect(first).resolves.toMatchObject({
      runtime: { state: "Running", paused: false },
    });
    await expect(second).resolves.toMatchObject({
      runtime: { state: "Running", paused: false },
    });
    expect(bridge.calls("get_snapshot")).toHaveLength(2);
  });

  it("notifies subscribers and removes the listener on unsubscribe", () => {
    const bridge = createMockHostBridge();
    const handler = vi.fn();
    const unsubscribe = bridge.subscribe("snapshot.changed", handler);

    bridge.emit("snapshot.changed", { reason: "runtime.refresh" });
    expect(handler).toHaveBeenCalledOnce();
    expect(handler).toHaveBeenCalledWith({ reason: "runtime.refresh" });

    unsubscribe();
    bridge.emit("snapshot.changed", { reason: "runtime.refresh" });
    expect(handler).toHaveBeenCalledOnce();
  });

  it("allows tests to inject a typed command error", async () => {
    const bridge = createMockHostBridge({
      errors: {
        get_snapshot: {
          code: "runtime_unavailable",
          message: "运行时尚未启动，请稍后重试",
          retryable: true,
        },
      },
    });

    await expect(bridge.invoke("get_snapshot", {})).rejects.toMatchObject({
      code: "runtime_unavailable",
      message: "运行时尚未启动，请稍后重试",
      retryable: true,
    });
  });

  it("allows tests to inject command latency", async () => {
    vi.useFakeTimers();
    const bridge = createMockHostBridge({
      delays: { get_snapshot: 50 },
    });

    const result = bridge.invoke("get_snapshot", {});
    let settled = false;
    void result.then(() => {
      settled = true;
    });

    await vi.advanceTimersByTimeAsync(49);
    expect(settled).toBe(false);

    await vi.advanceTimersByTimeAsync(1);
    await expect(result).resolves.toMatchObject({ runtime: { state: "Running" } });
    vi.useRealTimers();
  });
});
