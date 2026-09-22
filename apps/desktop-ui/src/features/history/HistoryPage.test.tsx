import { QueryClientProvider } from "@tanstack/react-query";
import {
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it } from "vitest";

import { createMockHostBridge } from "../../bridge";
import type { MockHostBridge } from "../../bridge";
import type {
  AgentDto,
  ChannelDto,
  NotificationDetailDto,
  NotificationSummaryDto,
} from "../../bridge/types";
import { createQueryClient } from "../../data/queryClient";
import { HistoryPage } from "./HistoryPage";

function notificationFixture(
  index: number,
  overrides: Partial<NotificationSummaryDto> = {},
): NotificationSummaryDto {
  return {
    id: `notification-${index}`,
    agentId: "agent-alpha",
    sessionId: `session-${index}`,
    sessionTitle: `会话 ${index}`,
    title: `通知 ${index}`,
    preview: `正文摘要 ${index}`,
    occurredAt: `2026-09-19T${String(index % 24).padStart(2, "0")}:00:00Z`,
    deliveryStates: ["Sent"],
    ...overrides,
  };
}

function agentFixture(): AgentDto {
  return {
    id: "agent-alpha",
    displayName: "Alpha Agent",
    description: "测试 Agent",
    configSchema: { type: "object", properties: {} },
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
  };
}

function channelsFixture(): ChannelDto[] {
  return [
    {
      id: "channel-alpha",
      displayName: "测试渠道",
      configSchema: { type: "object", properties: {} },
      capabilities: {
        sendText: true,
        receive: true,
        replyRouting: true,
        editMessage: false,
        attachments: false,
        markdown: false,
        maxTextBytes: null,
        inboundModes: ["LocalEvent"],
      },
      accounts: [
        {
          id: "account-alpha",
          channelId: "channel-alpha",
          displayName: "主账号",
          enabled: true,
          config: {},
          health: {
            available: true,
            stale: false,
            detail: null,
          },
          lastInboundAt: null,
          lastDeliveryAt: null,
        },
      ],
    },
  ];
}

function detailFixture(
  notification: NotificationSummaryDto,
  overrides: Partial<NotificationDetailDto> = {},
): NotificationDetailDto {
  return {
    notification,
    body: "这是仅在展开后显示的正文。",
    metadata: {},
    deliveries: [
      {
        id: "delivery-1",
        notificationId: notification.id,
        channelId: "channel-alpha",
        accountId: "account-alpha",
        state: "Sent",
        externalMessageId: "external-1",
        error: null,
        retryable: false,
        updatedAt: "2026-09-19T00:00:00Z",
      },
    ],
    routeExists: true,
    ...overrides,
  };
}

function renderHistory(bridge: MockHostBridge) {
  return render(
    <QueryClientProvider client={createQueryClient()}>
      <HistoryPage bridge={bridge} />
    </QueryClientProvider>,
  );
}

describe("HistoryPage", () => {
  it("shows Unknown with an instruction instead of automatic retry", async () => {
    const user = userEvent.setup();
    const notification = notificationFixture(1, {
      deliveryStates: ["Unknown"],
    });
    const bridge = createMockHostBridge({
      agents: [agentFixture()],
      channels: channelsFixture(),
      notifications: [notification],
    });

    renderHistory(bridge);

    await user.selectOptions(await screen.findByLabelText("状态"), "Unknown");

    expect(await screen.findByText("投递结果未确认")).toBeVisible();
    expect(screen.queryByRole("button", { name: "自动重试" })).not.toBeInTheDocument();
    expect(
      screen.getByText("请先检查原渠道是否已收到消息"),
    ).toBeVisible();
    expect(screen.queryByRole("button", { name: "重试" })).not.toBeInTheDocument();
  });

  it("sends every supported filter to the history command", async () => {
    const user = userEvent.setup();
    const bridge = createMockHostBridge({
      agents: [agentFixture()],
      channels: channelsFixture(),
      notifications: [notificationFixture(1)],
    });

    renderHistory(bridge);
    await screen.findByText("通知 1");

    await user.selectOptions(screen.getByLabelText("Agent"), "agent-alpha");
    await user.selectOptions(screen.getByLabelText("渠道"), "channel-alpha");
    await user.selectOptions(screen.getByLabelText("账号"), "account-alpha");
    await user.selectOptions(screen.getByLabelText("状态"), "Sent");
    fireEvent.change(screen.getByLabelText("开始时间"), {
      target: { value: "2026-09-19T08:00" },
    });
    fireEvent.change(screen.getByLabelText("结束时间"), {
      target: { value: "2026-09-19T09:00" },
    });
    await user.type(screen.getByLabelText("关键词"), "project alpha");

    await waitFor(() => {
      expect(bridge.calls("list_notifications").at(-1)?.payload).toEqual({
        agentId: "agent-alpha",
        channelId: "channel-alpha",
        accountId: "account-alpha",
        deliveryState: "Sent",
        from: new Date("2026-09-19T08:00").toISOString(),
        to: new Date("2026-09-19T09:00").toISOString(),
        query: "project alpha",
        cursor: null,
        limit: 10_000,
      });
    });
  });

  it("virtualizes 10,000 summaries without rendering every row", async () => {
    const notifications = Array.from({ length: 10_000 }, (_, index) =>
      notificationFixture(index + 1),
    );
    const bridge = createMockHostBridge({ notifications });

    renderHistory(bridge);

    expect(await screen.findByText("通知 1")).toBeVisible();
    const table = screen.getByRole("table", { name: "历史通知列表" });
    expect(within(table).getAllByRole("row").length).toBeLessThan(200);
    expect(screen.queryByText("通知 10000")).not.toBeInTheDocument();
  });

  it("shows detail existence and only exposes an explicit retry for Failed", async () => {
    const user = userEvent.setup();
    const notification = notificationFixture(1);
    const detail = detailFixture(notification, {
      routeExists: false,
      deliveries: [
        {
          id: "delivery-failed",
          notificationId: notification.id,
          channelId: "channel-alpha",
          accountId: "account-alpha",
          state: "Failed",
          externalMessageId: null,
          error: {
            code: "channel_rejected",
            message: "渠道拒绝了这条消息，请检查账号权限",
          },
          retryable: true,
          updatedAt: "2026-09-19T00:00:00Z",
        },
      ],
    });
    const bridge = createMockHostBridge({
      notifications: [notification],
      notificationDetails: { [notification.id]: detail },
    });

    renderHistory(bridge);
    await user.click(await screen.findByRole("button", { name: "通知 1" }));

    expect(await screen.findByText("通知")).toBeVisible();
    expect(screen.getByText("投递")).toBeVisible();
    expect(screen.getByText("存在（1 条）")).toBeVisible();
    expect(screen.getByText("路由")).toBeVisible();
    expect(screen.getByText("不存在")).toBeVisible();
    expect(
      within(
        screen.getByRole("region", { name: "通知详情" }),
      ).getByText("渠道拒绝了这条消息，请检查账号权限"),
    ).toBeVisible();
    expect(screen.queryByText("这是仅在展开后显示的正文。")).not.toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "展开正文" }));
    expect(screen.getByText("这是仅在展开后显示的正文。")).toBeVisible();

    await user.click(screen.getByRole("button", { name: "重试" }));
    await waitFor(() => {
      expect(bridge.calls("retry_delivery")).toEqual([
        {
          command: "retry_delivery",
          payload: { deliveryId: "delivery-failed" },
        },
      ]);
    });
  });

  it("does not offer retry when the selected delivery is Unknown", async () => {
    const user = userEvent.setup();
    const notification = notificationFixture(1, {
      deliveryStates: ["Unknown"],
    });
    const detail = detailFixture(notification, {
      deliveries: [
        {
          id: "delivery-unknown",
          notificationId: notification.id,
          channelId: "channel-alpha",
          accountId: "account-alpha",
          state: "Unknown",
          externalMessageId: null,
          error: {
            code: "delivery_result_unknown",
            message: "原始错误不应直接显示",
          },
          retryable: false,
          updatedAt: "2026-09-19T00:00:00Z",
        },
      ],
    });
    const bridge = createMockHostBridge({
      notifications: [notification],
      notificationDetails: { [notification.id]: detail },
    });

    renderHistory(bridge);
    await user.click(await screen.findByRole("button", { name: "通知 1" }));

    const detailRegion = await screen.findByRole("region", {
      name: "通知详情",
    });
    // 头部概览与投递卡片都会显示中文状态，因此用 getAllByText 断言存在。
    expect(
      within(detailRegion).getAllByText("投递结果未确认").length,
    ).toBeGreaterThan(0);
    expect(
      await within(detailRegion).findByText(/请先检查原渠道/),
    ).toBeVisible();
    expect(screen.queryByRole("button", { name: "重试" })).not.toBeInTheDocument();
    expect(screen.queryByText("原始错误不应直接显示")).not.toBeInTheDocument();
  });

  it("does not offer retry when a Failed delivery is not retryable", async () => {
    const user = userEvent.setup();
    const notification = notificationFixture(1, { deliveryStates: ["Failed"] });
    const detail = detailFixture(notification, {
      deliveries: [
        {
          id: "delivery-failed-final",
          notificationId: notification.id,
          channelId: "channel-alpha",
          accountId: "account-alpha",
          state: "Failed",
          externalMessageId: null,
          error: {
            code: "channel_rejected",
            message: "账号权限不足，需要人工处理",
          },
          retryable: false,
          updatedAt: "2026-09-19T00:00:00Z",
        },
      ],
    });
    const bridge = createMockHostBridge({
      notifications: [notification],
      notificationDetails: { [notification.id]: detail },
    });

    renderHistory(bridge);
    await user.click(await screen.findByRole("button", { name: "通知 1" }));

    const detailRegion = await screen.findByRole("region", {
      name: "通知详情",
    });
    expect(
      within(detailRegion).getByText("账号权限不足，需要人工处理"),
    ).toBeVisible();
    expect(screen.queryByRole("button", { name: "重试" })).not.toBeInTheDocument();
    expect(bridge.calls("retry_delivery")).toHaveLength(0);
  });
});
