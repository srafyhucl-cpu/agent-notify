import { QueryClientProvider } from "@tanstack/react-query";
import {
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it } from "vitest";

import { createMockHostBridge } from "../../bridge";
import type { MockHostBridge } from "../../bridge";
import type {
  ChannelAccountDto,
  ChannelDto,
  LoginSessionDto,
} from "../../bridge/types";
import { createQueryClient } from "../../data/queryClient";
import { ChannelsPage } from "./ChannelsPage";

const QR_DATA_URL =
  "[image omitted]";

function accountFixture(
  id: string,
  overrides: Partial<ChannelAccountDto> = {},
): ChannelAccountDto {
  return {
    id,
    channelId: "future-channel",
    displayName: `账号 ${id}`,
    enabled: true,
    config: {
      endpoint: `https://${id}.example.test`,
      pluginOptions: {
        preserve: `${id}-unknown`,
      },
    },
    health: {
      available: true,
      stale: false,
      detail: null,
    },
    lastInboundAt: "2026-09-19T08:00:00Z",
    lastDeliveryAt: "2026-09-19T08:01:00Z",
    ...overrides,
  };
}

function channelFixture(
  accounts: ChannelAccountDto[] = [],
  overrides: Partial<ChannelDto> = {},
): ChannelDto {
  return {
    id: "future-channel",
    displayName: "未来渠道",
    configSchema: {
      type: "object",
      properties: {
        endpoint: {
          type: "string",
          title: "服务地址",
        },
        pluginOptions: {
          type: "object",
          title: "插件选项",
        },
      },
    },
    capabilities: {
      sendText: true,
      receive: true,
      replyRouting: true,
      editMessage: false,
      attachments: false,
      markdown: false,
      maxTextBytes: 64,
      inboundModes: ["LocalEvent"],
    },
    accounts,
    ...overrides,
  };
}

function loginSessionFixture(
  overrides: Partial<LoginSessionDto> = {},
): LoginSessionDto {
  return {
    id: "login-session-1",
    accountId: null,
    accountKey: "account-key-1",
    state: "QrReady",
    qrPayload: QR_DATA_URL,
    createdAt: "2026-09-19T00:00:00Z",
    message: "请使用渠道客户端扫码",
    error: null,
    ...overrides,
  };
}

function renderChannels(bridge: MockHostBridge) {
  const queryClient = createQueryClient();
  return render(
    <QueryClientProvider client={queryClient}>
      <ChannelsPage bridge={bridge} />
    </QueryClientProvider>,
  );
}

describe("ChannelsPage", () => {
  it("renders descriptors and account controls without fixed channel branches", async () => {
    const account = accountFixture("account-a");
    const bridge = createMockHostBridge({
      channels: [channelFixture([account])],
    });

    renderChannels(bridge);

    expect(await screen.findByText("未来渠道")).toBeVisible();
    expect(screen.getByText("账号 account-a")).toBeVisible();
    expect(screen.getByText("服务正常")).toBeVisible();
    expect(
      screen.getByRole("switch", { name: "账号 account-a 启用" }),
    ).toBeChecked();
  });

  it("shows stale and unavailable account health with actionable messages", async () => {
    const stale = accountFixture("account-stale", {
      health: {
        available: true,
        stale: true,
        detail: {
          code: "clawbot_session_stale",
          message: "ClawBot 会话已失效，请重新扫码或发送消息恢复",
        },
      },
    });
    const invalid = accountFixture("account-invalid", {
      health: {
        available: false,
        stale: false,
        detail: {
          code: "clawbot_invalid_account",
          message: "ClawBot 登录状态已失效，请重新扫码",
        },
      },
    });
    const disabled = accountFixture("account-disabled", { enabled: false });
    const bridge = createMockHostBridge({
      channels: [channelFixture([stale, invalid, disabled])],
    });

    renderChannels(bridge);

    function rowFor(name: string): HTMLElement {
      const row = screen.getByText(name).closest("tr");
      if (!row) {
        throw new Error(`找不到账号行：${name}`);
      }
      return row as HTMLElement;
    }

    expect(await screen.findByText("账号 account-stale")).toBeVisible();
    const staleRow = rowFor("账号 account-stale");
    expect(within(staleRow).getByText("状态已过期")).toBeVisible();
    expect(
      within(staleRow).getByText("ClawBot 会话已失效，请重新扫码或发送消息恢复"),
    ).toBeVisible();

    const invalidRow = rowFor("账号 account-invalid");
    expect(within(invalidRow).getByText("登录异常")).toBeVisible();
    expect(
      within(invalidRow).getByText("ClawBot 登录状态已失效，请重新扫码"),
    ).toBeVisible();

    const disabledRow = rowFor("账号 account-disabled");
    expect(within(disabledRow).getByText("已停用")).toBeVisible();
  });

  it("isolates account details and refetches when the selected account changes", async () => {
    const user = userEvent.setup();
    const first = accountFixture("account-a");
    const second = accountFixture("account-b", {
      config: {
        endpoint: "https://second.example.test",
        pluginOptions: {
          preserve: "second-unknown",
        },
      },
    });
    const bridge = createMockHostBridge({
      channels: [channelFixture([first, second])],
    });

    renderChannels(bridge);

    const firstButton = await screen.findByRole("button", {
      name: "账号 account-a",
    });
    const secondButton = screen.getByRole("button", {
      name: "账号 account-b",
    });
    await waitFor(() => {
      expect(firstButton).toHaveAttribute("aria-pressed", "true");
    });
    const callsBeforeSwitch = bridge.calls("list_channel_accounts").length;

    await user.click(secondButton);

    expect(
      await screen.findByRole("heading", { name: "账号 account-b 详情" }),
    ).toBeVisible();
    expect(screen.queryByText("account-a-unknown")).not.toBeInTheDocument();
    await waitFor(() => {
      expect(bridge.calls("list_channel_accounts").length).toBeGreaterThan(
        callsBeforeSwitch,
      );
    });
  });

  it("moves from QR to paired without reloading the page", async () => {
    const user = userEvent.setup();
    const bridge = createMockHostBridge({
      channels: [channelFixture()],
    });

    renderChannels(bridge);
    await user.click(
      await screen.findByRole("button", { name: "添加渠道账号" }),
    );

    const qrCode = await screen.findByAltText("渠道登录二维码");
    expect(qrCode).toBeVisible();
    expect(qrCode.getAttribute("src")).toMatch(/^data:image\//);

    bridge.emit("channel.login.changed", {
      accountId: "account-1",
      state: "Paired",
      message: "登录成功，等待首条入站消息",
    });

    expect(
      await screen.findByText("登录成功，等待首条入站消息"),
    ).toBeVisible();
  });

  it("clears a verification code immediately and restores the active session after close", async () => {
    const user = userEvent.setup();
    const bridge = createMockHostBridge({
      channels: [channelFixture()],
      loginSession: loginSessionFixture({
        state: "NeedVerifyCode",
        message: "请输入配对码",
      }),
    });

    renderChannels(bridge);
    await user.click(
      await screen.findByRole("button", { name: "添加渠道账号" }),
    );

    const codeInput = await screen.findByLabelText("配对码");
    expect(codeInput).toHaveAttribute("type", "password");
    await user.type(codeInput, "123456");
    await user.click(screen.getByRole("button", { name: "提交配对码" }));

    expect(codeInput).toHaveValue("");
    await waitFor(() => {
      expect(bridge.calls("submit_channel_login_code")).toHaveLength(1);
    });
    expect(bridge.calls("submit_channel_login_code")[0]?.payload).toEqual({
      sessionId: "login-session-1",
      code: "123456",
    });

    await user.click(screen.getByRole("button", { name: "关闭登录窗口" }));
    await user.click(screen.getByRole("button", { name: "添加渠道账号" }));

    expect(await screen.findByText("验证码已提交，等待首条入站消息")).toBeVisible();
    expect(screen.queryByRole("button", { name: "提交配对码" })).not.toBeInTheDocument();
  });

  it("requires an explicit account for test notifications", async () => {
    const user = userEvent.setup();
    const first = accountFixture("account-a");
    const second = accountFixture("account-b");
    const bridge = createMockHostBridge({
      channels: [channelFixture([first, second])],
    });

    renderChannels(bridge);
    const sendForm = await screen.findByRole("form", { name: "测试发送" });
    const accountSelect = within(sendForm).getByLabelText("测试发送账号");
    const sendButton = within(sendForm).getByRole("button", {
      name: "发送测试通知",
    });

    expect(accountSelect).toHaveValue("");
    expect(sendButton).toBeDisabled();

    await user.selectOptions(accountSelect, "account-b");
    await user.type(within(sendForm).getByLabelText("标题"), "测试标题");
    await user.type(within(sendForm).getByLabelText("正文"), "测试正文");
    await user.click(sendButton);

    await waitFor(() => {
      expect(bridge.calls("send_test_notification")).toHaveLength(1);
    });
    expect(bridge.calls("send_test_notification")[0]?.payload).toEqual({
      accountId: "account-b",
      title: "测试标题",
      body: "测试正文",
    });
  });

  it("explains routing impact before logout and sends the selected account ID", async () => {
    const user = userEvent.setup();
    const bridge = createMockHostBridge({
      channels: [channelFixture([accountFixture("account-a")])],
    });

    renderChannels(bridge);
    await user.click(
      await screen.findByRole("button", { name: "退出账号 账号 account-a" }),
    );

    expect(await screen.findByRole("alertdialog")).toHaveTextContent(
      "路由和游标会失效，历史保留",
    );
    await user.click(screen.getByRole("button", { name: "确认退出" }));

    await waitFor(() => {
      expect(bridge.calls("logout_channel_account")).toHaveLength(1);
    });
    expect(bridge.calls("logout_channel_account")[0]?.payload).toEqual({
      accountId: "account-a",
    });
  });

  it("renders account config through SchemaForm without exposing a lossy replacement path", async () => {
    const bridge = createMockHostBridge({
      channels: [channelFixture([accountFixture("account-a")])],
    });

    renderChannels(bridge);

    expect(await screen.findByText("当前版本无法编辑此字段")).toBeVisible();
    expect(screen.queryByRole("button", { name: "保存配置" })).not.toBeInTheDocument();
  });

  it("keeps login sessions isolated by channel", async () => {
    const user = userEvent.setup();
    const bridge = createMockHostBridge({
      channels: [
        channelFixture([], { id: "channel-a", displayName: "渠道 A" }),
        channelFixture([], { id: "channel-b", displayName: "渠道 B" }),
      ],
    });

    renderChannels(bridge);
    const addButtons = await screen.findAllByRole("button", {
      name: "添加渠道账号",
    });

    await user.click(addButtons[0]!);
    await screen.findByAltText("渠道登录二维码");
    await waitFor(() => {
      expect(bridge.calls("begin_channel_login")).toHaveLength(1);
    });
    expect(bridge.calls("begin_channel_login")[0]?.payload).toMatchObject({
      channelId: "channel-a",
    });

    await user.click(screen.getByRole("button", { name: "关闭登录窗口" }));
    await user.click(addButtons[1]!);
    await waitFor(() => {
      expect(bridge.calls("begin_channel_login")).toHaveLength(2);
    });
    expect(bridge.calls("begin_channel_login")[1]?.payload).toMatchObject({
      channelId: "channel-b",
    });

    await user.click(screen.getByRole("button", { name: "关闭登录窗口" }));
    await user.click(addButtons[0]!);
    expect(await screen.findByAltText("渠道登录二维码")).toBeVisible();
    expect(bridge.calls("begin_channel_login")).toHaveLength(2);
  });
});
