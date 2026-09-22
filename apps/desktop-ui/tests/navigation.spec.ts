import { expect, test } from "@playwright/test";

import {
  defaultScenario,
  gotoSection,
  openHarness,
  SECTION_PATHS,
  SECTIONS,
} from "./helpers";

test.describe("导航流程", () => {
  test("可以从总览进入全部页面", async ({ page }) => {
    await openHarness(page, defaultScenario());

    for (const section of SECTIONS) {
      if (section === "总览") {
        continue;
      }
      await gotoSection(page, section);
      await expect(page).toHaveURL(
        new RegExp(`${SECTION_PATHS[section]}$`),
      );
    }

    await gotoSection(page, "总览");
    await expect(page).toHaveURL(/\/overview$/);
  });

  test("键盘 Tab 按导航顺序移动并用 Enter 打开页面", async ({ page }) => {
    await openHarness(page, defaultScenario());

    const expectedOrder = ["AgentNotify 总览", ...SECTIONS];
    for (const name of expectedOrder) {
      await page.keyboard.press("Tab");
      await expect(page.locator(":focus")).toHaveAccessibleName(name);
    }

    await page.keyboard.press("Enter");
    await expect(
      page.getByRole("heading", { level: 1, name: "设置" }),
    ).toBeVisible();
    await expect(page).toHaveURL(/\/settings$/);
  });
});
