import { expect, test } from "@playwright/test";

import { countInvocations, gotoSection, openHarness } from "./helpers";

test.describe("交互一致性（整块可点）", () => {
  test("Agent 卡片：头部整块可点、chevron 可点、开关不误触折叠、键盘仍可用", async ({
    page,
  }) => {
    await openHarness(page);
    await gotoSection(page, "Agent 管理");

    const firstCard = page.locator(".agent-accordion-card").first();
    const titleButton = firstCard.getByRole("button", {
      name: "Alpha Agent",
      exact: true,
    });
    const header = firstCard.locator(".agent-card-header");

    // 默认展开首张卡片（与渠道页默认选中首个账号一致）
    await expect(titleButton).toHaveAttribute("aria-pressed", "true");

    // ① 点头部内边距区域（不是标题文字）即可收起——修复前只有标题文字可点
    await header.click({ position: { x: 8, y: 8 } });
    await expect(titleButton).toHaveAttribute("aria-pressed", "false");

    // ② 点右侧 chevron 即可再次展开
    await firstCard.locator(".agent-card-chevron").click();
    await expect(titleButton).toHaveAttribute("aria-pressed", "true");

    // ③ 点通知开关不触发折叠（stopPropagation 生效），且切换命令恰好调用一次
    //    可见的开关本体是 .switch-track，input 只是 1×1 的语义载体
    await firstCard.locator(".switch-track").click();
    await expect(titleButton).toHaveAttribute("aria-pressed", "true");
    expect(await countInvocations(page, "update_agent_config")).toBe(1);

    // ④ 键盘路径仍然可用：聚焦标题按钮按 Enter 收起
    await titleButton.focus();
    await page.keyboard.press("Enter");
    await expect(titleButton).toHaveAttribute("aria-pressed", "false");
  });
});
