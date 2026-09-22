import { expect, test } from "@playwright/test";

import {
  countInvocations,
  defaultScenario,
  gotoSection,
  openHarness,
} from "./helpers";
import {
  notificationDetailFixture,
  notificationFixture,
} from "../src/test/fixtures";

test.describe("状态与恢复动作", () => {
  test("Unknown 投递只显示检查指引，不提供重试", async ({ page }) => {
    const scenario = defaultScenario();
    const notification = notificationFixture(1, {
      title: "结果未知的通知",
      deliveryStates: ["Unknown"],
    });

    await openHarness(page, {
      ...scenario,
      notifications: [notification],
      notificationDetails: {
        [notification.id]: notificationDetailFixture(notification, {
          deliveries: [
            {
              id: "delivery-unknown",
              notificationId: notification.id,
              channelId: "future-channel",
              accountId: "account-a",
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
        }),
      },
    });

    await gotoSection(page, "历史");
    await page.getByRole("button", { name: "结果未知的通知" }).click();

    const detailRegion = page.getByRole("region", { name: "通知详情" });
    await expect(
      detailRegion.getByText("投递结果未确认", { exact: true }).first(),
    ).toBeVisible();
    await expect(
      detailRegion.getByText(/请先检查原渠道/).first(),
    ).toBeVisible();
    await expect(page.getByRole("button", { name: "重试" })).toHaveCount(0);
    await expect(page.getByText("原始错误不应直接显示")).toHaveCount(0);
  });

  test("Failed 且 retryable 的投递可以显式重试一次", async ({ page }) => {
    await openHarness(page, defaultScenario());

    await gotoSection(page, "历史");
    await page.getByRole("button", { name: "投递失败" }).click();
    await expect(page.getByText("渠道拒绝了这条消息，请检查账号权限")).toBeVisible();

    await page.getByRole("button", { name: "重试" }).click();
    await expect
      .poll(() => countInvocations(page, "retry_delivery"))
      .toBe(1);
  });

  test("诊断修复动作调用 descriptor 声明的命令", async ({ page }) => {
    await openHarness(page, defaultScenario());
    await gotoSection(page, "诊断");

    await expect(page.getByText("渠道登录状态异常")).toBeVisible();
    const before = await countInvocations(page, "get_snapshot");

    await page.getByRole("button", { name: "重新读取运行状态" }).click();

    await expect
      .poll(() => countInvocations(page, "get_snapshot"))
      .toBeGreaterThan(before);
  });

  test("渠道读取失败时给出可执行的重试动作", async ({ page }) => {
    const scenario = defaultScenario();
    await openHarness(page, {
      ...scenario,
      errors: {
        list_channel_accounts: {
          code: "channel_list_failed",
          message: "渠道列表暂时不可用",
          retryable: true,
        },
      },
    });

    await gotoSection(page, "渠道");
    await expect(page.getByText("无法读取渠道账号")).toBeVisible();
    await expect(page.getByText("渠道列表暂时不可用")).toBeVisible();

    const before = await countInvocations(page, "list_channel_accounts");
    await page.getByRole("button", { name: "重新检查" }).click();

    await expect
      .poll(() => countInvocations(page, "list_channel_accounts"))
      .toBeGreaterThan(before);
  });

  test("没有 Agent 时显示空态而不是占位页面", async ({ page }) => {
    await openHarness(page, { ...defaultScenario(), agents: [] });
    await gotoSection(page, "Agent 管理");

    await expect(
      page.getByRole("heading", { level: 2, name: "暂无 Agent" }),
    ).toBeVisible();
    await expect(page.getByText(/安装或启用 Agent 适配器后/)).toBeVisible();
  });
});
