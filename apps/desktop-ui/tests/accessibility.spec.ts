import AxeBuilder from "@axe-core/playwright";
import { expect, test } from "@playwright/test";

import { channelFixture } from "../src/test/fixtures";
import { defaultScenario, gotoSection, openHarness, SECTIONS } from "./helpers";

const BLOCKING_IMPACTS = new Set(["serious", "critical"]);

test.describe("可访问性", () => {
  for (const section of SECTIONS) {
    test(`@a11y ${section} 没有严重可访问性问题`, async ({ page }) => {
      await openHarness(page, defaultScenario());
      await gotoSection(page, section);
      await page.waitForLoadState("networkidle");

      const results = await new AxeBuilder({ page }).analyze();
      const blocking = results.violations.filter((violation) =>
        BLOCKING_IMPACTS.has(violation.impact ?? ""),
      );

      expect(
        blocking.map((violation) =>
          [
            `${violation.id}: ${violation.help}（${String(violation.nodes.length)} 处）`,
            ...violation.nodes.map((node) => `  - ${node.target.join(" ")}`),
          ].join("\n"),
        ),
      ).toEqual([]);
    });
  }

  test("@a11y 登录对话框支持 Escape 关闭并恢复焦点", async ({ page }) => {
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
    const addAccount = page.getByRole("button", { name: "添加渠道账号" });
    await addAccount.click();

    const dialog = page.getByRole("dialog", { name: "登录 未来渠道" });
    await expect(dialog).toBeVisible();
    await expect(page.getByRole("button", { name: "关闭登录窗口" })).toBeFocused();

    await page.keyboard.press("Escape");
    await expect(dialog).toBeHidden();
  });
});
