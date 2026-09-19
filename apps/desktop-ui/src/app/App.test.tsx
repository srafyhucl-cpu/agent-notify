import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import { createMockHostBridge } from "../bridge";
import { App } from "./App";

describe("App", () => {
  it("renders the desktop workbench shell", () => {
    render(<App bridge={createMockHostBridge()} />);

    expect(screen.getByRole("main", { name: "AgentNotify 工作台" })).toBeInTheDocument();
    expect(screen.getByText("AgentNotify")).toBeInTheDocument();
  });
});
