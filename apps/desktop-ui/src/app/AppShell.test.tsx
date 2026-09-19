import { render, screen, waitFor, within } from "@testing-library/react";
import { QueryClientProvider } from "@tanstack/react-query";
import userEvent from "@testing-library/user-event";
import { MemoryRouter } from "react-router-dom";
import { describe, expect, it } from "vitest";

import { createMockHostBridge } from "../bridge";
import type { BusinessCommand, CommandPayloadMap } from "../bridge";
import type { RuntimeSnapshotDto } from "../bridge/types";
import { EmptyState } from "../components/EmptyState";
import { InlineError } from "../components/InlineError";
import { LoadingRows } from "../components/LoadingRows";
import { createQueryClient } from "../data/queryClient";
import { AppRouter } from "./router";
import { AppShell } from "./AppShell";
import { navigationItems } from "./navigation";

function runtimeSnapshot(paused = false): RuntimeSnapshotDto {
  return {
    runtime: {
      appVersion: "2.0.0-dev.0",
      platform: "windows",
      state: paused ? "Paused" : "Running",
      paused,
    },
    overview: {
      storage: {
        notificationCount: 0,
        deliveryCount: 0,
        pendingOutboxCount: 0,
        recentError: null,
      },
      agents: [],
      channels: [],
      recentDeliveries: [],
    },
    components: [],
    diagnostics: [],
  };
}

function createStatefulRuntimeBridge() {
  const snapshot = runtimeSnapshot();
  const bridge = createMockHostBridge({ snapshot });
  const invoke = bridge.invoke.bind(bridge);

  bridge.invoke = async <TCommand extends BusinessCommand>(
    command: TCommand,
    payload: CommandPayloadMap[TCommand],
  ) => {
    const result = await invoke(command, payload);
    if (command === "get_snapshot") {
      return structuredClone(snapshot) as typeof result;
    }
    if (command === "set_runtime_paused") {
      const paused = (
        payload as CommandPayloadMap["set_runtime_paused"]
      ).paused;
      snapshot.runtime = {
        ...snapshot.runtime,
        state: paused ? "Paused" : "Running",
        paused,
      };
      return { ...snapshot.runtime } as typeof result;
    }
    return result;
  };

  return bridge;
}

function renderShell(path = "/overview") {
  return render(
    <MemoryRouter initialEntries={[path]}>
      <AppRouter bridge={createMockHostBridge()} />
    </MemoryRouter>,
  );
}

describe("AppShell", () => {
  it("exposes all primary destinations with keyboard-readable names", async () => {
    renderAppShell();

    for (const item of navigationItems) {
      expect(screen.getByRole("link", { name: item.label })).toBeVisible();
    }
    expect(screen.getByRole("main")).toHaveAttribute("id", "main-content");
  });

  it("renders every stable route", async () => {
    for (const item of navigationItems) {
      const view = renderShell(item.path);

      expect(
        await screen.findByRole("heading", { level: 1, name: item.label }),
      ).toBeVisible();
      view.unmount();
    }
  });

  it("shows runtime state and version from the host snapshot", async () => {
    const bridge = createMockHostBridge({
      snapshot: {
        runtime: {
          appVersion: "9.8.7",
          platform: "windows",
          state: "Running",
          paused: false,
        },
        overview: {
          storage: {
            notificationCount: 0,
            deliveryCount: 0,
            pendingOutboxCount: 0,
            recentError: null,
          },
          agents: [],
          channels: [],
          recentDeliveries: [],
        },
        components: [],
        diagnostics: [],
      },
    });

    renderAppShell(bridge);

    expect(await screen.findByText("运行中")).toBeVisible();
    expect(screen.getByText("版本 9.8.7")).toBeVisible();
    expect(screen.getByRole("button", { name: "暂停通知" })).toBeEnabled();
  });

  it("pauses runtime through HostBridge and keeps the control accessible", async () => {
    const user = userEvent.setup();
    const bridge = createStatefulRuntimeBridge();
    renderAppShell(bridge);

    await user.click(await screen.findByRole("button", { name: "暂停通知" }));

    await waitFor(() => {
      expect(bridge.calls("set_runtime_paused")).toEqual([
        { command: "set_runtime_paused", payload: { paused: true } },
      ]);
    });
    expect(screen.getByRole("button", { name: "恢复通知" })).toBeEnabled();
  });

  it("shows an actionable error when runtime status cannot be loaded", async () => {
    const user = userEvent.setup();
    const bridge = createMockHostBridge({
      errors: {
        get_snapshot: {
          code: "runtime_unavailable",
          message: "运行时尚未启动",
          retryable: true,
        },
      },
    });

    renderAppShell(bridge);

    expect(await screen.findByRole("alert")).toHaveTextContent("无法读取运行状态");
    expect(screen.getByRole("alert")).toHaveTextContent("请重新检查");
    await user.click(screen.getByRole("button", { name: "重新检查" }));
    expect(bridge.calls("get_snapshot")).toHaveLength(2);
  });

  it("renders reusable empty, error, and loading states", () => {
    const { rerender } = render(
      <EmptyState
        description="当前没有需要处理的项目"
        action={<button type="button">创建第一项</button>}
      />,
    );
    expect(screen.getByText("当前没有需要处理的项目")).toBeVisible();
    expect(screen.getByRole("button", { name: "创建第一项" })).toBeVisible();

    rerender(
      <InlineError
        title="操作未完成"
        message="请检查连接后重试"
        action={<button type="button">重试</button>}
      />,
    );
    expect(screen.getByRole("alert")).toHaveTextContent("操作未完成");
    expect(screen.getByRole("button", { name: "重试" })).toBeVisible();

    rerender(<LoadingRows aria-label="正在加载列表" rows={3} />);
    expect(screen.getByRole("status", { name: "正在加载列表" })).toBeVisible();
    expect(screen.getByRole("status").children).toHaveLength(3);
  });
});
function renderAppShell(bridge = createMockHostBridge()) {
  const queryClient = createQueryClient();
  return render(
    <QueryClientProvider client={queryClient}>
      <MemoryRouter>
        <AppShell bridge={bridge} />
      </MemoryRouter>
    </QueryClientProvider>,
  );
}

function overviewRuntimeRegion(): HTMLElement {
  return screen
    .getByRole("heading", { name: "运行状态" })
    .closest("section") as HTMLElement;
}
describe("AppShell runtime synchronization", () => {
  it("updates Overview when the top status pauses runtime", async () => {
    const user = userEvent.setup();
    const bridge = createStatefulRuntimeBridge();
    const view = render(
      <MemoryRouter initialEntries={["/overview"]}>
        <AppRouter bridge={bridge} />
      </MemoryRouter>,
    );

    const statusBar = await screen.findByRole("region", {
      name: "运行时状态",
    });
    await screen.findByRole("heading", { name: "运行状态" });
    const overviewRuntime = overviewRuntimeRegion();

    await user.click(
      within(statusBar).getByRole("button", { name: "暂停通知" }),
    );

    await waitFor(() => {
      expect(within(statusBar).getByText("已暂停")).toBeVisible();
      expect(
        within(overviewRuntime).getByRole("button", { name: "恢复通知" }),
      ).toBeEnabled();
    });
    view.unmount();
  });

  it("updates the top status when Overview pauses runtime", async () => {
    const user = userEvent.setup();
    const bridge = createStatefulRuntimeBridge();
    const view = render(
      <MemoryRouter initialEntries={["/overview"]}>
        <AppRouter bridge={bridge} />
      </MemoryRouter>,
    );

    const statusBar = await screen.findByRole("region", {
      name: "运行时状态",
    });
    await screen.findByRole("heading", { name: "运行状态" });
    const overviewRuntime = overviewRuntimeRegion();

    await user.click(
      within(overviewRuntime).getByRole("button", { name: "暂停通知" }),
    );

    await waitFor(() => {
      expect(within(statusBar).getByText("已暂停")).toBeVisible();
      expect(
        within(statusBar).getByRole("button", { name: "恢复通知" }),
      ).toBeEnabled();
    });
    view.unmount();
  });
});
