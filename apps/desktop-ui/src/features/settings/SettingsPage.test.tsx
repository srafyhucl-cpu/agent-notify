import { QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it } from "vitest";

import { createMockHostBridge } from "../../bridge";
import type { MockHostBridge } from "../../bridge";
import type {
  AgentDto,
  ChannelDto,
  DiagnosticsDto,
  SettingsDto,
} from "../../bridge/types";
import { createQueryClient } from "../../data/queryClient";
import { legacyMigrationFixture } from "../../test/fixtures";
import { DiagnosticsPage } from "../diagnostics/DiagnosticsPage";
import { SettingsPage } from "./SettingsPage";

function settingsFixture(overrides: Partial<SettingsDto> = {}): SettingsDto {
  return {
    notificationsPaused: false,
    quietHours: null,
    cooldownSeconds: 0,
    defaultChannelAccountId: null,
    replyEnabled: true,
    deliveryReceiptEnabled: false,
    routeTtlSeconds: 86_400,
    autoStart: false,
    startHidden: false,
    updateChannel: "Stable",
    ...overrides,
  };
}

function channelsFixture(): ChannelDto[] {
  return [
    {
      id: "channel-alpha",
      displayName: "测试渠道",
      configSchema: {
        type: "object",
        properties: {
          endpoint: { type: "string", title: "服务地址" },
          apiKey: { type: "secret-string", title: "API Key" },
        },
      },
      capabilities: {
        sendText: true,
        receive: true,
        replyRouting: true,
        editMessage: false,
        attachments: false,
        markdown: false,
        maxTextBytes: null,
        inboundModes: ["LocalEvent"],
      },
      accounts: [
        {
          id: "account-alpha",
          channelId: "channel-alpha",
          displayName: "主账号",
          enabled: true,
          config: {
            endpoint: "https://example.test",
            apiKey: "already-stored",
          },
          health: {
            available: true,
            stale: false,
            detail: null,
          },
          lastInboundAt: null,
          lastDeliveryAt: null,
        },
      ],
    },
  ];
}

function agentFixture(): AgentDto {
  return {
    id: "agent-alpha",
    displayName: "Alpha Agent",
    description: "测试 Agent",
    configSchema: { type: "object", properties: {} },
    capabilities: {
      notify: true,
      resume: true,
      sessionTitle: true,
      hookInstaller: false,
      replyWindow: false,
    },
    enabled: true,
    config: {},
    health: {
      available: true,
      detail: null,
    },
  };
}

function diagnosticsFixture(): DiagnosticsDto {
  return {
    generatedAt: "2026-09-19T00:00:00Z",
    runtime: {
      appVersion: "2.0.0-dev.0",
      platform: "windows",
      state: "Running",
      paused: false,
    },
    storage: {
      notificationCount: 120,
      deliveryCount: 240,
      pendingOutboxCount: 1,
      recentError: null,
    },
    components: [],
    items: [
      {
        code: "database.integrity",
        level: "Error",
        message: "数据库完整性检查发现问题",
        checkedAt: "2026-09-19T00:00:00Z",
        action: {
          label: "重新检查",
          command: "get_diagnostics",
          payload: {},
        },
      },
      {
        code: "webview.version",
        level: "Normal",
        message: "WebView2 版本可用",
        checkedAt: "2026-09-19T00:00:00Z",
        action: null,
      },
    ],
    migration: legacyMigrationFixture(),
  };
}

function renderWithQuery(ui: React.ReactNode) {
  return render(
    <QueryClientProvider client={createQueryClient()}>{ui}</QueryClientProvider>,
  );
}

function renderSettings(bridge: MockHostBridge) {
  return renderWithQuery(<SettingsPage bridge={bridge} />);
}

describe("SettingsPage", () => {
  it("groups settings and marks commands without a stable business contract unavailable", async () => {
    const user = userEvent.setup();
    const bridge = createMockHostBridge({
      settings: settingsFixture(),
      channels: channelsFixture(),
      agents: [agentFixture()],
    });

    renderSettings(bridge);

    expect(
      await screen.findByRole("heading", { level: 2, name: "通知" }),
    ).toBeVisible();
    expect(screen.getByRole("heading", { level: 2, name: "回复" })).toBeVisible();
    expect(screen.getByRole("heading", { level: 2, name: "渠道" })).toBeVisible();
    expect(screen.getByRole("heading", { level: 2, name: "应用" })).toBeVisible();
    expect(screen.getByRole("heading", { level: 2, name: "数据" })).toBeVisible();
    expect(screen.getByRole("switch", { name: "全局暂停" })).not.toBeChecked();
    expect(
      screen.getByRole("option", { name: /主账号.*account-alpha/ }),
    ).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "打开数据目录" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "导出脱敏诊断包" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "备份数据库" })).toBeDisabled();
    expect(screen.getAllByText("当前版本不可用").length).toBeGreaterThanOrEqual(3);

    await user.click(screen.getByRole("button", { name: "检查更新" }));
    expect(
      await screen.findByText(
        "测试包未签名，仅供内部验证；正式版不会安装未签名更新。",
      ),
    ).toBeVisible();
    expect(bridge.calls("get_update_status")).toHaveLength(1);
  });

  it("validates, saves, and reports a successful settings update", async () => {
    const user = userEvent.setup();
    const bridge = createMockHostBridge({
      settings: settingsFixture(),
      channels: channelsFixture(),
      agents: [agentFixture()],
    });

    renderSettings(bridge);
    const paused = await screen.findByRole("switch", { name: "全局暂停" });
    await user.click(paused);

    const cooldown = screen.getByLabelText("通知冷却（秒）");
    await user.clear(cooldown);
    await user.type(cooldown, "30");
    await user.selectOptions(screen.getByLabelText("默认通知账号"), "account-alpha");
    await user.click(screen.getByRole("button", { name: "保存设置" }));

    await waitFor(() => {
      expect(bridge.calls("update_settings")).toEqual([
        {
          command: "update_settings",
          payload: expect.objectContaining({
            notificationsPaused: true,
            cooldownSeconds: 30,
            defaultChannelAccountId: "account-alpha",
          }),
        },
      ]);
    });
    expect(await screen.findByText("设置已保存")).toBeVisible();
  });

  it("rolls the draft back and keeps the saved value when saving fails", async () => {
    const user = userEvent.setup();
    const bridge = createMockHostBridge({
      settings: settingsFixture(),
      channels: channelsFixture(),
      agents: [agentFixture()],
      errors: {
        update_settings: {
          code: "settings_write_failed",
          message: "设置保存失败，请检查数据库状态。",
          retryable: false,
        },
      },
    });

    renderSettings(bridge);
    const paused = await screen.findByRole("switch", { name: "全局暂停" });
    await user.click(paused);
    expect(paused).toBeChecked();

    await user.click(screen.getByRole("button", { name: "保存设置" }));

    expect(await screen.findByRole("alert")).toHaveTextContent(
      "设置保存失败，请检查数据库状态",
    );
    expect(paused).not.toBeChecked();
  });

  it("blocks invalid numeric and quiet-hour values before calling the host", async () => {
    const user = userEvent.setup();
    const bridge = createMockHostBridge({
      settings: settingsFixture(),
      channels: channelsFixture(),
      agents: [agentFixture()],
    });

    renderSettings(bridge);
    await screen.findByRole("switch", { name: "全局暂停" });
    const cooldown = screen.getByLabelText("通知冷却（秒）");
    await user.clear(cooldown);
    await user.type(cooldown, "4000");
    await user.click(screen.getByRole("button", { name: "保存设置" }));

    expect(await screen.findByRole("alert")).toHaveTextContent(
      "通知冷却必须是 0 到 3600 之间的整数",
    );
    expect(bridge.calls("update_settings")).toHaveLength(0);
  });
});

describe("DiagnosticsPage", () => {
  it("renders StatusService items and invokes only their declared action", async () => {
    const user = userEvent.setup();
    const bridge = createMockHostBridge({
      diagnostics: diagnosticsFixture(),
    });

    renderWithQuery(<DiagnosticsPage bridge={bridge} />);

    expect(
      await screen.findByText("数据库完整性检查发现问题"),
    ).toBeVisible();
    expect(screen.getByText("异常")).toBeVisible();
    expect(screen.getByText("WebView2 版本可用")).toBeVisible();
    expect(screen.getByText("正常")).toBeVisible();
    expect(screen.getByText("当前没有可执行的修复动作")).toBeVisible();

    await user.click(screen.getByRole("button", { name: "重新检查" }));
    await waitFor(() => {
      expect(bridge.calls("get_diagnostics")).toHaveLength(3);
    });
  });
});
