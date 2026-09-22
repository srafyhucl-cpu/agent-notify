import { expect, test } from "@playwright/test";
import type { Page } from "@playwright/test";

import { longChineseText } from "../src/test/fixtures";

import {
  expectNoHorizontalOverflow,
  expectNoOverlap,
  expectNotClipped,
  gotoSection,
  longTextScenario,
  openHarness,
  SECTIONS,
  type SectionLabel,
} from "./helpers";

const SLUGS: Record<SectionLabel, string> = {
  总览: "overview",
  "Agent 管理": "agents",
  渠道: "channels",
  历史: "history",
  诊断: "diagnostics",
  设置: "settings",
};

async function waitForStableLayout(page: Page): Promise<void> {
  await page.evaluate(() => document.fonts.ready);
}

test.describe("视觉与布局", () => {
  for (const section of SECTIONS) {
    test(`@visual ${section} 视觉基线`, async ({ page }) => {
      await openHarness(page, longTextScenario());
      await gotoSection(page, section);
      await waitForStableLayout(page);

      await expectNoHorizontalOverflow(page);
      await expect(page).toHaveScreenshot(`${SLUGS[section]}.png`);
    });
  }

  test("@visual 长文本不被裁切且关键区域不重叠", async ({ page }) => {
    await openHarness(page, longTextScenario());
    await waitForStableLayout(page);

    for (const section of SECTIONS) {
      await expectNotClipped(
        page.getByRole("link", { name: section, exact: true }),
      );
    }

    await gotoSection(page, "Agent 管理");
    const agentName = page
      .getByRole("button", { name: longChineseText })
      .first();
    await expectNotClipped(agentName);
    await expectNoOverlap(
      agentName,
      page.getByRole("switch", { name: `${longChineseText} 通知` }),
    );

    await expectNotClipped(
      page.getByRole("heading", {
        level: 2,
        name: `${longChineseText} 详情`,
      }),
    );
    await expectNotClipped(
      page.getByText("服务可用", { exact: true }).first(),
    );

    await gotoSection(page, "渠道");
    await expectNotClipped(
      page.getByRole("button", { name: /^这是一个用于验证中文长文本/ }).first(),
    );
    await expectNoHorizontalOverflow(page);

    await gotoSection(page, "历史");
    await expectNotClipped(
      page.getByRole("button", { name: longChineseText }).first(),
    );
    await expectNoHorizontalOverflow(page);

    await gotoSection(page, "设置");
    for (const label of ["全局暂停", "通知冷却（秒）", "默认通知账号"]) {
      await expectNotClipped(page.getByLabel(label));
    }
  });

  test("@zoom 200% 缩放等价视口下无水平溢出", async ({ page }) => {
    await openHarness(page, longTextScenario());
    await waitForStableLayout(page);

    for (const section of SECTIONS) {
      await gotoSection(page, section);
      await expectNoHorizontalOverflow(page);
      await expectNotClipped(
        page.getByRole("link", { name: section, exact: true }),
      );
    }
  });
});
;
