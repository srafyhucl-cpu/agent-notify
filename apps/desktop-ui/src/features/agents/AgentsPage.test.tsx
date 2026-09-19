import { QueryClientProvider } from "@tanstack/react-query";
import {
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MemoryRouter } from "react-router-dom";
import { describe, expect, it } from "vitest";

import type {
  AgentDto,
  DeliveryDto,
  RuntimeSnapshotDto,
} from "../../bridge/types";
import { createMockHostBridge } from "../../bridge";
import type { MockHostBridge } from "../../bridge";
import { AppRouter } from "../../app/router";
import { createQueryClient } from "../../data/queryClient";
import { OverviewPage } from "../overview/OverviewPage";
import { AgentsPage } from "./AgentsPage";

function agentFixture(overrides: Partial<AgentDto> = {}): AgentDto {
  return {
    id: "future-agent",
    displayName: "Future Agent",
    description: "由 descriptor 动态接入的未来 Agent",
    configSchema: {
      type: "object",
      properties: {},
    },
    capabilities: {
      notify: true,
      resume: true,
      sessionTitle: true,
      hookInstaller: false,
      replyWindow: false,
    },
    enabled: true,
    config: {},
    health: {
      available: true,
      detail: null,
    },
    ...overrides,
  };
}

function deliveryFixture(index: number): DeliveryDto {
  return {
    id: `delivery-${index}`,
    notificationId: `notification-${index}`,
    channelId: "channel-future",
    accountId: "account-future",
    state: index === 1 ? "Failed" : "Sent",
    externalMessageId: null,
    error:
      index === 1
        ? {
            code: "channel_rejected",
            message: "渠道拒绝了这条消息，请检查账号权限",
          }
        : null,
    updatedAt: `2026-09-19T00:${String(index).padStart(2, "0")}:00Z`,
  };
}

function snapshotWithAgents(
  agents: AgentDto[],
  overrides: Partial<RuntimeSnapshotDto> = {},
): RuntimeSnapshotDto {
  return {
    runtime: {
      appVersion: "2.0.0-dev.0",
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
      agents,
      channels: [],
      recentDeliveries: [],
    },
    components: [],
    diagnostics: [],
    ...overrides,
  };
}

function renderWithQuery(ui: React.ReactNode) {
  const queryClient = createQueryClient();
  return render(
    <QueryClientProvider client={queryClient}>{ui}</QueryClientProvider>,
  );
}

function renderAgents(bridge: MockHostBridge) {
  return renderWithQuery(<AgentsPage bridge={bridge} />);
}

describe("AgentsPage", () => {
  it("renders a new agent without editing page business branches", async () => {
    const agent = agentFixture();
    const bridge = createMockHostBridge({
      agents: [agent],
    });

    renderAgents(bridge);

    await waitFor(() => {
      expect(bridge.calls("list_agents")).toHaveLength(1);
    });
    expect(await screen.findByText("Future Agent")).toBeVisible();
    expect(
      screen.getByRole("switch", { name: "Future Agent 通知" }),
    ).toBeChecked();
    expect(screen.getByText("回复")).toBeVisible();
    expect(screen.getByText("接入正常")).toBeVisible();
  });

  it("shows controls from capabilities instead of fixed agent IDs", async () => {
    const agent = agentFixture({
      id: "no-notify-agent",
      displayName: "No Notify Agent",
      capabilities: {
        notify: false,
        resume: false,
        sessionTitle: false,
        hookInstaller: false,
        replyWindow: false,
      },
    });
    const bridge = createMockHostBridge({
      agents: [agent],
    });

    renderAgents(bridge);

    expect(await screen.findByText("No Notify Agent")).toBeVisible();
    expect(
      screen.queryByRole("switch", { name: "No Notify Agent 通知" }),
    ).not.toBeInTheDocument();
    expect(screen.getByText("不支持")).toBeVisible();
  });

  it("preserves unsupported values and clears a submitted secret input", async () => {
    const user = userEvent.setup();
    const agent = agentFixture({
      id: "schema-agent",
      displayName: "Schema Agent",
      configSchema: {
        type: "object",
        properties: {
          endpoint: { type: "string", title: "服务地址" },
          apiKey: { type: "secret-string", title: "API Key" },
          enabled: { type: "boolean", title: "启用轮询" },
          retries: { type: "integer", title: "重试次数" },
          ratio: { type: "number", title: "采样比例" },
          mode: {
            type: "enum",
            title: "运行模式",
            enum: ["safe", "fast"],
          },
          prompt: { type: "textarea", title: "附加提示" },
          pluginOptions: { type: "object", title: "插件配置" },
        },
      },
      config: {
        endpoint: "https://example.test",
        apiKey: "already-stored",
        enabled: true,
        retries: 2,
        ratio: 0.5,
        mode: "safe",
        prompt: "保持简洁",
        pluginOptions: { preserve: "必须保留" },
      },
    });
    const bridge = createMockHostBridge({
      agents: [agent],
    });

    renderAgents(bridge);

    expect(await screen.findByText("Schema Agent")).toBeVisible();
    expect(screen.getByText("已配置")).toBeVisible();
    expect(screen.getByText("当前版本无法编辑此字段")).toBeVisible();
    const secretInput = screen.getByLabelText("API Key");
    expect(secretInput).toHaveValue("");

    await user.type(secretInput, "new-secret");
    const form = screen.getByRole("form", { name: "Schema Agent 配置" });
    await user.click(within(form).getByRole("button", { name: "保存配置" }));

    await waitFor(() => {
      expect(bridge.calls("update_agent_config")).toHaveLength(1);
    });
    expect(bridge.calls("update_agent_config")[0]?.payload).toEqual({
      agentId: "schema-agent",
      enabled: null,
      config: expect.objectContaining({
        apiKey: "new-secret",
        pluginOptions: { preserve: "必须保留" },
      }),
    });
    await waitFor(() => {
      expect(secretInput).toHaveValue("");
    });
  });

  it("keeps the original switch and shows an actionable error when update fails", async () => {
    const user = userEvent.setup();
    const agent = agentFixture({ displayName: "Failing Agent" });
    const bridge = createMockHostBridge({
      agents: [agent],
      errors: {
        update_agent_config: {
          code: "agent_update_failed",
          message: "配置保存失败，请检查 Agent 状态后重试。",
          retryable: false,
        },
      },
    });

    renderAgents(bridge);

    const toggle = await screen.findByRole("switch", {
      name: "Failing Agent 通知",
    });
    await user.click(toggle);

    expect(await screen.findByRole("alert")).toHaveTextContent(
      "请检查 Agent 状态后重试",
    );
    expect(toggle).toBeChecked();
  });
});

describe("OverviewPage", () => {
  it("renders the required information bands in the approved order", async () => {
    const deliveries = Array.from({ length: 21 }, (_, index) =>
      deliveryFixture(index + 1),
    );
    const bridge = createMockHostBridge({
      snapshot: snapshotWithAgents([agentFixture()], {
        overview: {
          storage: {
            notificationCount: 0,
            deliveryCount: 21,
            pendingOutboxCount: 0,
            recentError: null,
          },
          agents: [agentFixture()],
          channels: [],
          recentDeliveries: deliveries,
        },
      }),
    });

    renderWithQuery(<OverviewPage bridge={bridge} />);

    await screen.findByRole("heading", { name: "运行状态" });
    const headings = await screen.findAllByRole("heading");
    const orderedLabels = [
      "运行状态",
      "Agent 接入",
      "Channel 账号",
      "最近投递",
      "需要处理",
    ];
    const positions = orderedLabels.map((label) =>
      headings.findIndex((heading) => heading.textContent === label),
    );
    expect(positions.every((position) => position >= 0)).toBe(true);
    expect(positions).toEqual([...positions].sort((left, right) => left - right));
    expect(screen.getByText("notification-1")).toBeVisible();
    expect(screen.queryByText("notification-21")).not.toBeInTheDocument();
  });
});

describe("AppRouter host events", () => {
  it("subscribes at the route layer and refetches agents on snapshot changes", async () => {
    const bridge = createMockHostBridge({
      agents: [agentFixture()],
    });
    renderWithQuery(
      <MemoryRouter initialEntries={["/agents"]}>
        <AppRouter bridge={bridge} />
      </MemoryRouter>,
    );

    await screen.findByText("Future Agent");
    await waitFor(() => {
      expect(bridge.calls("list_agents").length).toBeGreaterThan(0);
    });
    const initialCalls = bridge.calls("list_agents").length;

    bridge.emit("snapshot.changed", { reason: "test.refresh" });

    await waitFor(() => {
      expect(bridge.calls("list_agents").length).toBeGreaterThan(initialCalls);
    });
  });
});
  it("preserves a configured secret when the draft input is left blank", async () => {
    const user = userEvent.setup();
    const agent = agentFixture({
      id: "secret-agent",
      displayName: "Secret Agent",
      configSchema: {
        type: "object",
        properties: {
          apiKey: { type: "secret-string", title: "API Key" },
        },
      },
      config: {
        apiKey: "already-stored",
      },
    });
    const bridge = createMockHostBridge({
      agents: [agent],
    });

    renderAgents(bridge);

    expect(await screen.findByText("Secret Agent")).toBeVisible();
    const form = screen.getByRole("form", { name: "Secret Agent 配置" });
    await user.click(within(form).getByRole("button", { name: "保存配置" }));

    await waitFor(() => {
      expect(bridge.calls("update_agent_config")).toHaveLength(1);
    });
    expect(bridge.calls("update_agent_config")[0]?.payload).toEqual({
      agentId: "secret-agent",
      enabled: null,
      config: {
        apiKey: "already-stored",
      },
    });
  });
