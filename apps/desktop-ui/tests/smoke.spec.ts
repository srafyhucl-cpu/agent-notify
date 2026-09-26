import { expect, test } from "@playwright/test";

import { defaultScenario, gotoSection, openHarness, SECTIONS } from "./helpers";

/**
 * 控件冒烟：逐页真实点击所有可用控件，断言全程没有 console 错误与未捕获异常。
 *
 * 起因：用户问过"每个按钮都能用吗"。渲染 / 无障碍 / 视觉三层门禁都覆盖不到"点了会不会报错"，
 * 这条用 mock 场景（无真实渠道）把六页 + 状态栏 + 侧栏的控件都点一遍。
 *
 * 只点按钮与主题选项：开关本体是 1×1 的透明 input（点击目标是它的可见轨道），
 * 由 tests/interactions.spec.ts 覆盖；导航链接由 navigation.spec.ts 覆盖。
 */
const CONTROL_SELECTOR = [
  "main button:visible",
  ".runtime-status-bar button:visible",
  ".app-nav button:visible",
  ".theme-switcher-option:visible",
].join(", ");

// 冒烟要逐页真实点击所有控件，在负载较高的机器上可能接近默认 30s 上限；
// 放宽到 60s，避免环境抖动造成假失败。
test.setTimeout(60_000);

test("控件冒烟：逐页点击所有可用控件，不产生 console 错误与 JS 异常", async ({
  page,
}) => {
  const errors: string[] = [];
  page.on("console", (message) => {
    if (message.type() === "error") {
      errors.push(`console: ${message.text().slice(0, 200)}`);
    }
  });
  page.on("pageerror", (error) => {
    errors.push(`pageerror: ${String(error).slice(0, 200)}`);
  });

  await openHarness(page, defaultScenario());

  let clicked = 0;
  for (const section of SECTIONS) {
    await gotoSection(page, section);

    const controls = page.locator(CONTROL_SELECTOR);
    const total = await controls.count();
    for (let index = 0; index < total; index++) {
      const control = controls.nth(index);
      const label = (
        (await control.getAttribute("aria-label")) ??
        (await control.textContent()) ??
        "?"
      )
        .replace(/\s+/g, " ")
        .trim()
        .slice(0, 28);

      // 禁用态不可点（例如没有未保存更改时的「保存设置」），跳过而不是当成失败
      if (!(await control.isEnabled())) {
        continue;
      }

      try {
        await control.click({ timeout: 3000 });
        clicked += 1;
      } catch (error) {
        errors.push(`CLICK-FAIL [${section}] "${label}": ${String(error).slice(0, 160)}`);
      }
      // 点开的对话框一律善后，避免遮罩挡住后续控件
      await page.keyboard.press("Escape");
      await page.waitForTimeout(60);
    }
  }

  // 实测约 66 个（六页 + 状态栏 + 侧栏）；低于 10 说明选择器或场景失效，而不是界面变简单
  expect(
    clicked,
    `实际只点到 ${String(clicked)} 个控件，覆盖数量明显偏低，检查选择器或场景`,
  ).toBeGreaterThan(10);
  expect(errors, `控件冒烟发现 ${String(errors.length)} 个问题`).toEqual([]);
});
