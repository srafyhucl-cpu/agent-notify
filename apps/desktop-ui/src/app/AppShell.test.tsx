import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MemoryRouter } from "react-router-dom";
import { describe, expect, it } from "vitest";

import { createMockHostBridge } from "../bridge";
import { EmptyState } from "../components/EmptyState";
import { InlineError } from "../components/InlineError";
import { LoadingRows } from "../components/LoadingRows";
import { AppRouter } from "./router";
import { AppShell } from "./AppShell";
import { navigationItems } from "./navigation";

function renderShell(path = "/overview") {
  return render(
    <MemoryRouter initialEntries={[path]}>
      <AppRouter bridge={createMockHostBridge()} />
    </MemoryRouter>,
  );
}

describe("AppShell", () => {
  it("exposes all primary destinations with keyboard-readable names", async () => {
    render(
      <MemoryRouter initialEntries={["/overview"]}>
        <AppShell bridge={createMockHostBridge()} />
      </MemoryRouter>,
    );

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

    render(
      <MemoryRouter>
        <AppShell bridge={bridge} />
      </MemoryRouter>,
    );

    expect(await screen.findByText("运行中")).toBeVisible();
    expect(screen.getByText("版本 9.8.7")).toBeVisible();
    expect(screen.getByRole("button", { name: "暂停通知" })).toBeEnabled();
  });

  it("pauses runtime through HostBridge and keeps the control accessible", async () => {
    const user = userEvent.setup();
    const bridge = createMockHostBridge();
    render(
      <MemoryRouter>
        <AppShell bridge={bridge} />
      </MemoryRouter>,
    );

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

    render(
      <MemoryRouter>
        <AppShell bridge={bridge} />
      </MemoryRouter>,
    );

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
