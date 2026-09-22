import { screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import { createMockHostBridge } from "../bridge";
import { AppRouter } from "./router";
import { renderWithBridge } from "../test/renderWithBridge";
import { twoAgents } from "../test/fixtures";

const ROUTES = [
  { route: "/overview", heading: "总览" },
  { route: "/agents", heading: "Agent 管理" },
  { route: "/channels", heading: "渠道" },
  { route: "/history", heading: "历史" },
  { route: "/diagnostics", heading: "诊断" },
  { route: "/settings", heading: "设置" },
] as const;

describe("AppRouter", () => {
  it.each(ROUTES)(
    "renders $heading at $route without a placeholder",
    async ({ route, heading }) => {
      const bridge = createMockHostBridge({ agents: twoAgents });

      renderWithBridge(<AppRouter bridge={bridge} />, { route });

      expect(
        await screen.findByRole("heading", { level: 1, name: heading }),
      ).toBeVisible();
      expect(screen.queryByText("当前没有可显示的内容。")).not.toBeInTheDocument();
    },
  );

  it("redirects unknown routes to the overview page", async () => {
    const bridge = createMockHostBridge({ agents: twoAgents });

    renderWithBridge(<AppRouter bridge={bridge} />, { route: "/unknown" });

    expect(
      await screen.findByRole("heading", { level: 1, name: "总览" }),
    ).toBeVisible();
  });
});
