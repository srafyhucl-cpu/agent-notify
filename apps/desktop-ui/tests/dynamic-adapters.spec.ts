import { expect, test } from "@playwright/test";

import { channelFixture } from "../src/test/fixtures";
import {
  countInvocations,
  defaultScenario,
  emitHostEvent,
  gotoSection,
  openHarness,
} from "./helpers";

test.describe("descriptor 驱动的动态适配器", () => {
  test("未知 ID 的 Agent 与渠道按 descriptor 自动出现", async ({ page }) => {
    await openHarness(page, defaultScenario());

    await gotoSection(page, "Agent 管理");
    await expect(
      page.getByRole("button", { name: "Alpha Agent", exact: true }),
    ).toBeVisible();
    await expect(
      page.getByRole("button", { name: "未来 Agent", exact: true }),
    ).toBeVisible();

    await gotoSection(page, "渠道");
    await expect(
      page.getByRole("heading", { level: 2, name: "未来渠道" }),
    ).toBeVisible();
    await expect(
      page.getByRole("button", { name: "主账号", exact: true }),
    ).toBeVisible();
  });

  test("渠道登录从二维码推进到等待入站并最终配对", async ({ page }) => {
    const scenario = defaultScenario();
    await openHarness(page, {
      ...scenario,
      channels: [
        channelFixture("future-channel", {
          displayName: "未来渠道",
          accounts: [],
        }),
      ],
    });

    await gotoSection(page, "渠道");
    await page.getByRole("button", { name: "添加渠道账号" }).click();

    const dialog = page.getByRole("dialog", { name: "登录 未来渠道" });
    await expect(dialog).toBeVisible();
    await expect(page.getByAltText("渠道登录二维码")).toBeVisible();
    await expect(dialog.getByText("二维码已就绪")).toBeVisible();

    await emitHostEvent(page, "channel.login.changed", {
      accountId: "",
      sessionId: "login-session-1",
      state: "NeedVerifyCode",
      message: "请输入渠道客户端显示的配对码",
    });

    await page.getByLabel("配对码").fill("123456");
    await page.getByRole("button", { name: "提交配对码" }).click();
    await expect(
      dialog.getByText("等待首条入站消息", { exact: true }).first(),
    ).toBeVisible();

    await emitHostEvent(page, "channel.login.changed", {
      accountId: "account-new",
      sessionId: "login-session-1",
      state: "Paired",
      message: "账号已经可以接收与发送消息",
    });

    await expect(dialog.getByText("登录成功")).toBeVisible();
    await page.getByRole("button", { name: "完成" }).click();
    await expect(dialog).toBeHidden();
  });

  test("测试发送收进账号行：一键发送，内置内容带账号名", async ({ page }) => {
    await openHarness(page, defaultScenario());
    await gotoSection(page, "渠道");

    // 旧面板已下线：不再需要选账号/填标题正文
    await expect(page.getByRole("form", { name: "测试发送" })).toHaveCount(0);

    await page.getByRole("button", { name: "测试发送 主账号" }).click();

    await expect
      .poll(() => countInvocations(page, "send_test_notification"))
      .toBe(1);
    const payload = await page.evaluate(
      () =>
        window.__AGENT_NOTIFY_BRIDGE__?.calls("send_test_notification")[0]
          ?.payload ?? null,
    );
    expect(payload).toMatchObject({ accountId: "account-a", title: "测试通知" });
    expect(String((payload as { body?: string }).body)).toContain("主账号");
    // 结果用浮层提示（不占版面），文案固定
    await expect(page.getByRole("status")).toContainText("已发送测试消息。");
  });
});
