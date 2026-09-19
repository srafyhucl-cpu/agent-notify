import { QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it } from "vitest";

import { createMockHostBridge } from "../../bridge";
import type { MockHostBridge } from "../../bridge";
import { createQueryClient } from "../../data/queryClient";
import {
  diagnosticsFixture,
  legacyMigrationFixture,
} from "../../test/fixtures";
import { DiagnosticsPage } from "./DiagnosticsPage";

function renderDiagnostics(bridge: MockHostBridge) {
  return render(
    <QueryClientProvider client={createQueryClient()}>
      <DiagnosticsPage bridge={bridge} />
    </QueryClientProvider>,
  );
}

describe("MigrationDiagnostics", () => {
  it("shows the not-detected state without exposing migration actions", async () => {
    const bridge = createMockHostBridge({
      diagnostics: diagnosticsFixture({
        migration: legacyMigrationFixture({ state: "NotDetected" }),
      }),
    });

    renderDiagnostics(bridge);

    const section = await screen.findByRole("region", { name: "旧数据迁移" });
    expect(within(section).getByText("未发现旧数据")).toBeVisible();
    expect(
      within(section).getByRole("button", { name: "查看迁移报告" }),
    ).toBeDisabled();
    expect(
      within(section).getByRole("button", { name: "重新检测" }),
    ).toBeEnabled();
  });

  it("shows skipped records and opens the migration report", async () => {
    const user = userEvent.setup();
    const bridge = createMockHostBridge({
      diagnostics: diagnosticsFixture({
        migration: legacyMigrationFixture({
          state: "Partial",
          sourceDetected: true,
          reportFile: "D:\\Temp\\agentnotify-migration.json",
          report: {
            importedAt: "2026-09-19T10:00:00Z",
            sourceFileCount: 3,
            settingsImported: 2,
            agentConfigsImported: 4,
            accountsImported: 1,
            notificationsImported: 12,
            deliveriesImported: 12,
            routesImported: 8,
            claimsImported: 3,
            skippedRecords: 2,
            warnings: [
              {
                code: "invalid_json",
                file: "push.log",
                record: 7,
              },
            ],
          },
        }),
      }),
    });

    renderDiagnostics(bridge);

    const section = await screen.findByRole("region", { name: "旧数据迁移" });
    expect(
      within(section).getByText("部分完成，跳过 2 条损坏记录"),
    ).toBeVisible();
    await user.click(
      within(section).getByRole("button", { name: "查看迁移报告" }),
    );

    const report = within(section).getByRole("list", {
      name: "迁移损坏警告",
    });
    expect(within(report).getByText("invalid_json")).toBeVisible();
    expect(within(report).getByText("push.log")).toBeVisible();
  });

  it("shows failure details, backup guidance, and retries detection", async () => {
    const user = userEvent.setup();
    const bridge = createMockHostBridge({
      diagnostics: diagnosticsFixture({
        migration: legacyMigrationFixture({
          state: "Required",
          sourceDetected: true,
          error: {
            code: "legacy_import_failed",
            message: "clawbot.json 中的账号凭据格式无效",
            file: "D:\\Config\\clawbot.json",
            field: "accounts[0].token",
          },
        }),
      }),
    });

    renderDiagnostics(bridge);

    const section = await screen.findByRole("region", { name: "旧数据迁移" });
    expect(
      within(section).getByText("失败，当前处于只读诊断模式"),
    ).toBeVisible();
    expect(
      within(section).getByText("clawbot.json 中的账号凭据格式无效"),
    ).toBeVisible();
    expect(
      within(section).getByText(
        "备份建议：请先将旧版数据目录和现有文件完整复制到安全位置，再处理迁移问题并重新检测。迁移完成前请保留原文件。",
      ),
    ).toBeVisible();

    await user.click(
      within(section).getByRole("button", { name: "重新检测" }),
    );

    await waitFor(() => {
      expect(bridge.calls("retry_legacy_migration")).toHaveLength(1);
    });
  });
});
