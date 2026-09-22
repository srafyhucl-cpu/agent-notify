import { expect } from "@playwright/test";
import type { Locator, Page } from "@playwright/test";

import type { MockHostBridgeOptions } from "../src/bridge";
import type {
  BusinessCommand,
  EventPayloadMap,
  HostEvent,
} from "../src/bridge";
import {
  accountFixture,
  channelFixture,
  diagnosticItemFixture,
  diagnosticsFixture,
  longChineseText,
  longUnbrokenText,
  notificationDetailFixture,
  notificationFixture,
  settingsFixture,
  twoAgents,
} from "../src/test/fixtures";

export const HARNESS_PATH = "/tests/harness/";

export const SECTIONS = [
  "总览",
  "Agent 管理",
  "渠道",
  "历史",
  "诊断",
  "设置",
] as const;

export type SectionLabel = (typeof SECTIONS)[number];

/** 导航标签到路由路径的稳定映射（标签用于可访问名，路径用于 URL 断言）。 */
export const SECTION_PATHS: Record<SectionLabel, string> = {
  总览: "/overview",
  "Agent 管理": "/agents",
  渠道: "/channels",
  历史: "/history",
  诊断: "/diagnostics",
  设置: "/settings",
};

/** 稳定的默认场景：覆盖六个页面、两种投递状态和一条可执行诊断动作。 */
export function defaultScenario(): MockHostBridgeOptions {
  const sent = notificationFixture(1, { title: "构建完成", preview: "流水线已经通过" });
  const failed = notificationFixture(2, {
    title: "投递失败",
    preview: "渠道拒绝了这条消息",
    deliveryStates: ["Failed"],
  });

  return {
    agents: twoAgents,
    channels: [
      channelFixture("future-channel", {
        displayName: "未来渠道",
        accounts: [
          accountFixture("account-a", {
            channelId: "future-channel",
            displayName: "主账号",
          }),
        ],
      }),
    ],
    notifications: [sent, failed],
    notificationDetails: {
      [sent.id]: notificationDetailFixture(sent, { routeExists: true }),
      [failed.id]: notificationDetailFixture(failed, {
        routeExists: false,
        deliveries: [
          {
            id: "delivery-failed",
            notificationId: failed.id,
            channelId: "future-channel",
            accountId: "account-a",
            state: "Failed",
            externalMessageId: null,
            error: {
              code: "channel_rejected",
              message: "渠道拒绝了这条消息，请检查账号权限",
            },
            retryable: true,
            updatedAt: "2026-09-19T02:00:00Z",
          },
        ],
      }),
    },
    diagnostics: diagnosticsFixture({
      items: [
        diagnosticItemFixture("storage.integrity", {
          message: "数据库迁移与完整性检查通过",
        }),
        diagnosticItemFixture("channel.login", {
          level: "Error",
          message: "渠道登录状态异常，需要重新读取运行时状态",
          action: {
            label: "重新读取运行状态",
            command: "get_snapshot",
            payload: {},
          },
        }),
      ],
    }),
    settings: settingsFixture(),
  };
}

/** 长文本场景：用于验证布局不裁切、不重叠。 */
export function longTextScenario(): MockHostBridgeOptions {
  const scenario = defaultScenario();
  const notification = notificationFixture(1, {
    title: longChineseText,
    preview: `${longUnbrokenText} ${longChineseText}`,
  });

  return {
    ...scenario,
    agents: [
      {
        ...twoAgents[0]!,
        displayName: longChineseText,
        description: `${longUnbrokenText}${longChineseText}`,
      },
      twoAgents[1]!,
    ],
    channels: [
      channelFixture("future-channel", {
        displayName: `${longChineseText} ${longUnbrokenText}`,
        accounts: [
          accountFixture("account-a", {
            channelId: "future-channel",
            displayName: `${longChineseText}（${longUnbrokenText}）`,
            health: {
              available: false,
              stale: false,
              detail: {
                code: "channel_rejected",
                message: `${longChineseText} ${longUnbrokenText}`,
              },
            },
          }),
        ],
      }),
    ],
    notifications: [notification],
    notificationDetails: {
      [notification.id]: notificationDetailFixture(notification, {
        deliveries: [
          {
            id: "delivery-long",
            notificationId: notification.id,
            channelId: "future-channel",
            accountId: "account-a",
            state: "Failed",
            externalMessageId: null,
            error: {
              code: "channel_rejected",
              message: `${longChineseText} ${longUnbrokenText}`,
            },
            retryable: true,
            updatedAt: "2026-09-19T00:00:00Z",
          },
        ],
      }),
    },
  };
}

export async function openHarness(
  page: Page,
  scenario: MockHostBridgeOptions = defaultScenario(),
): Promise<void> {
  await page.addInitScript((value) => {
    window.__AGENT_NOTIFY_HARNESS__ = { scenario: value };
  }, scenario);
  await page.goto(HARNESS_PATH);
  await expect(
    page.getByRole("heading", { level: 1, name: "总览" }),
  ).toBeVisible();
}

export async function gotoSection(
  page: Page,
  section: SectionLabel,
): Promise<void> {
  await page.getByRole("link", { name: section, exact: true }).click();
  await expect(
    page.getByRole("heading", { level: 1, name: section }),
  ).toBeVisible();
}

export async function emitHostEvent<TEvent extends HostEvent>(
  page: Page,
  event: TEvent,
  payload: EventPayloadMap[TEvent],
): Promise<void> {
  await page.evaluate(
    ({ eventName, eventPayload }) => {
      const bridge = window.__AGENT_NOTIFY_BRIDGE__;
      if (!bridge) {
        throw new Error("UI 测试桥尚未就绪");
      }
      (bridge.emit as (name: HostEvent, value: unknown) => void)(
        eventName,
        eventPayload,
      );
    },
    { eventName: event, eventPayload: payload },
  );
}

export async function countInvocations(
  page: Page,
  command: BusinessCommand,
): Promise<number> {
  return page.evaluate((name) => {
    const bridge = window.__AGENT_NOTIFY_BRIDGE__;
    if (!bridge) {
      throw new Error("UI 测试桥尚未就绪");
    }
    return bridge.calls(name).length;
  }, command);
}

/** 断言元素自身没有被水平裁切。 */
export async function expectNotClipped(locator: Locator): Promise<void> {
  await expect(locator).toBeVisible();
  const metrics = await locator.evaluate((element) => ({
    scrollWidth: element.scrollWidth,
    clientWidth: element.clientWidth,
  }));
  expect(
    metrics.scrollWidth,
    `元素被水平裁切：scrollWidth=${String(metrics.scrollWidth)} clientWidth=${String(metrics.clientWidth)}`,
  ).toBeLessThanOrEqual(metrics.clientWidth + 1);
}

/** 断言文档没有产生水平滚动条。 */
export async function expectNoHorizontalOverflow(page: Page): Promise<void> {
  // .app-main 自身可滚动，必须同时检查，否则内部横向滚动会被 documentElement 掩盖。
  const overflowing = await page.evaluate(() =>
    [document.documentElement, document.querySelector(".app-main")]
      .filter((element): element is Element => element !== null)
      .filter((element) => element.scrollWidth > element.clientWidth + 1)
      .map((element) => ({
        selector: element === document.documentElement ? ":root" : ".app-main",
        scrollWidth: element.scrollWidth,
        clientWidth: element.clientWidth,
      })),
  );
  expect(
    overflowing,
    `页面出现水平溢出：${JSON.stringify(overflowing)}`,
  ).toEqual([]);
}

/** 断言两个元素的外框不重叠。 */
export async function expectNoOverlap(
  first: Locator,
  second: Locator,
): Promise<void> {
  const firstBox = await first.boundingBox();
  const secondBox = await second.boundingBox();
  expect(firstBox, "第一个元素没有可见外框").not.toBeNull();
  expect(secondBox, "第二个元素没有可见外框").not.toBeNull();
  if (!firstBox || !secondBox) {
    return;
  }

  const separated =
    firstBox.x + firstBox.width <= secondBox.x ||
    secondBox.x + secondBox.width <= firstBox.x ||
    firstBox.y + firstBox.height <= secondBox.y ||
    secondBox.y + secondBox.height <= firstBox.y;
  expect(separated, "元素外框发生重叠").toBe(true);
}
