import {
  keepPreviousData,
  QueryClientProvider,
  type QueryClient,
} from "@tanstack/react-query";
import { render, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { createMockHostBridge } from "../bridge";
import type { MockHostBridge } from "../bridge";
import { toUserError } from "./errors";
import {
  createQueryClient,
  DIAGNOSTICS_STALE_TIME_MS,
  HISTORY_STALE_TIME_MS,
  SNAPSHOT_STALE_TIME_MS,
} from "./queryClient";
import { queryKeys } from "./queryKeys";
import { useHostEvent } from "./useHostEvent";

function HostEventHarness({
  bridge,
  queryClient,
}: {
  bridge: MockHostBridge;
  queryClient: QueryClient;
}) {
  useHostEvent(bridge);

  return (
    <QueryClientProvider client={queryClient}>
      <span>ready</span>
    </QueryClientProvider>
  );
}

function renderHostEvents(bridge: MockHostBridge, queryClient: QueryClient) {
  return render(
    <QueryClientProvider client={queryClient}>
      <HostEventHarness bridge={bridge} queryClient={queryClient} />
    </QueryClientProvider>,
  );
}

describe("query cache policy", () => {
  it("uses the approved stale times and disables mutation retries", () => {
    expect(SNAPSHOT_STALE_TIME_MS).toBe(5_000);
    expect(HISTORY_STALE_TIME_MS).toBe(30_000);
    expect(DIAGNOSTICS_STALE_TIME_MS).toBe(Number.POSITIVE_INFINITY);
    expect(createQueryClient().getDefaultOptions().mutations?.retry).toBe(false);
  });

  it("does not retry business errors and keeps previous data", () => {
    const queryDefaults = createQueryClient().getDefaultOptions().queries;
    expect(queryDefaults?.placeholderData).toBe(keepPreviousData);
    expect(typeof queryDefaults?.retry).toBe("function");

    if (typeof queryDefaults?.retry === "function") {
      const businessError = {
        code: "runtime_unavailable",
        message: "运行时尚未启动",
        retryable: true,
      } as unknown as Error;
      expect(
        queryDefaults.retry(0, businessError),
      ).toBe(false);
      expect(queryDefaults.retry(0, new Error("network unavailable"))).toBe(true);
    }
  });
});

describe("host event invalidation", () => {
  it("invalidates snapshot, deliveries, history, and detail when delivery.changed arrives", async () => {
    const bridge = createMockHostBridge();
    const queryClient = createQueryClient();
    const invalidate = vi.spyOn(queryClient, "invalidateQueries");
    renderHostEvents(bridge, queryClient);

    bridge.emit("delivery.changed", {
      deliveryId: "delivery-1",
      notificationId: "notification-1",
      state: "Sent",
    });

    await waitFor(() => {
      expect(invalidate).toHaveBeenCalledWith({
        queryKey: queryKeys.snapshot(),
      });
      expect(invalidate).toHaveBeenCalledWith({
        queryKey: queryKeys.deliveries(),
      });
      expect(invalidate).toHaveBeenCalledWith({
        queryKey: queryKeys.notifications(),
      });
      expect(invalidate).toHaveBeenCalledWith({
        queryKey: queryKeys.notificationDetail("notification-1"),
      });
    });
  });

  it("invalidates the snapshot when snapshot.changed arrives", async () => {
    const bridge = createMockHostBridge();
    const queryClient = createQueryClient();
    const invalidate = vi.spyOn(queryClient, "invalidateQueries");
    renderHostEvents(bridge, queryClient);

    bridge.emit("snapshot.changed", { reason: "runtime.refresh" });

    await waitFor(() => {
      expect(invalidate).toHaveBeenCalledWith({
        queryKey: queryKeys.snapshot(),
      });
    });
  });

  it("invalidates channels and login sessions when channel.login.changed arrives", async () => {
    const bridge = createMockHostBridge();
    const queryClient = createQueryClient();
    const invalidate = vi.spyOn(queryClient, "invalidateQueries");
    renderHostEvents(bridge, queryClient);

    bridge.emit("channel.login.changed", {
      accountId: "account-1",
      sessionId: "login-session-1",
      state: "Paired",
      message: "登录成功",
    });

    await waitFor(() => {
      expect(invalidate).toHaveBeenCalledWith({
        queryKey: queryKeys.snapshot(),
      });
      expect(invalidate).toHaveBeenCalledWith({
        queryKey: queryKeys.channels(),
      });
      expect(invalidate).toHaveBeenCalledWith({
        queryKey: queryKeys.channelLogin("account-1"),
      });
    });
  });
});

describe("toUserError", () => {
  it("never exposes a raw 401 response", () => {
    const error = toUserError({
      code: "http_401",
      message: "401 Unauthorized: authorization=Bearer raw-token",
      retryable: false,
      diagnosticId: "diag-401",
    });

    expect(error.message).toContain("重新登录");
    expect(error.message).not.toContain("raw-token");
    expect(error.message).not.toContain("authorization");
    expect(error.diagnosticId).toBe("diag-401");
  });

  it("guides credential errors to login again", () => {
    const error = toUserError({
      code: "credential_invalid",
      message: "凭据已失效",
      retryable: false,
    });

    expect(error.message).toContain("重新登录");
    expect(error.action).toBeUndefined();
  });

  it("guides database errors to backup and Diagnostics", () => {
    const error = toUserError({
      code: "database_locked",
      message: "数据库暂时不可用",
      retryable: false,
    });

    expect(error.message).toContain("备份");
    expect(error.message).toContain("Diagnostics");
    expect(error.action).toEqual({
      label: "查看 Diagnostics",
      command: "get_diagnostics",
      payload: {},
    });
  });

  it("only asks the user to inspect the original channel for Unknown delivery", () => {
    const error = toUserError(
      {
        code: "delivery_result_unknown",
        message: "投递结果未确认",
        retryable: false,
      },
      { delivery: { id: "delivery-1", state: "Unknown" } },
    );

    expect(error.message).toContain("请先检查原渠道是否已收到消息");
    expect(error.message).not.toContain("重试");
    expect(error.action).toBeUndefined();
  });

  it("uses conservative copy when the business boundary cannot be determined", () => {
    const error = toUserError(new Error("501 upstream raw response body"));

    expect(error.title).toBe("操作未完成");
    expect(error.message).toContain("请稍后重试");
    expect(error.message).not.toContain("upstream raw response body");
  });
});
