import type {
  AgentDto,
  ChannelAccountDto,
  ChannelDto,
  DeliveryDto,
  DiagnosticItemDto,
  DiagnosticsDto,
  LegacyMigrationDto,
  NotificationDetailDto,
  NotificationSummaryDto,
  SettingsDto,
} from "../bridge/types";

/** 用于验证中文长文本、数字与错误信息换行的固定样本。 */
export const longChineseText =
  "这是一个用于验证中文长文本、数字 1234567890 与错误信息换行的测试标题";

export const longUnbrokenText =
  "agentnotify-very-long-adapter-identifier-1234567890-abcdefghijklmnopqrstuvwxyz";

export const agentCapabilities = {
  notify: true,
  resume: true,
  sessionTitle: true,
  hookInstaller: false,
  replyWindow: false,
} as const;

export function agentFixture(
  id: string,
  overrides: Partial<AgentDto> = {},
): AgentDto {
  return {
    id,
    displayName: `Agent ${id}`,
    description: "测试 Agent",
    configSchema: { type: "object", properties: {} },
    capabilities: { ...agentCapabilities },
    enabled: true,
    config: {},
    health: { available: true, detail: null },
    ...overrides,
  };
}

/** 尚未发现任何 Agent：用于空态断言。 */
export const noAgents: AgentDto[] = [];

/** 两个能力不同的 Agent：用于证明页面按 descriptor 渲染而不是写死 ID。 */
export const twoAgents: AgentDto[] = [
  agentFixture("alpha", { displayName: "Alpha Agent" }),
  agentFixture("future", {
    displayName: "未来 Agent",
    capabilities: { ...agentCapabilities, resume: false },
  }),
];

export function accountFixture(
  id: string,
  overrides: Partial<ChannelAccountDto> = {},
): ChannelAccountDto {
  return {
    id,
    channelId: "future-channel",
    displayName: `账号 ${id}`,
    enabled: true,
    config: {},
    health: { available: true, stale: false, detail: null },
    lastInboundAt: "2026-09-19T08:00:00Z",
    lastDeliveryAt: "2026-09-19T08:01:00Z",
    ...overrides,
  };
}

export function channelFixture(
  id: string,
  overrides: Partial<ChannelDto> = {},
): ChannelDto {
  return {
    id,
    displayName: `渠道 ${id}`,
    configSchema: {
      type: "object",
      properties: {
        endpoint: { type: "string", title: "服务地址" },
        pluginOptions: { type: "object", title: "插件选项" },
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
    accounts: [],
    ...overrides,
  };
}

/** 渠道账号的四种稳定健康状态。 */
export const channelStates = [
  {
    state: "NotLoggedIn",
    health: { available: false, stale: false, detail: null },
  },
  {
    state: "WaitingFirstInbound",
    health: { available: true, stale: true, detail: null },
  },
  {
    state: "Ready",
    health: { available: true, stale: false, detail: null },
  },
  {
    state: "Blocked",
    health: {
      available: false,
      stale: false,
      detail: { code: "channel_blocked", message: "渠道登录被阻止" },
    },
  },
] as const;

export function notificationFixture(
  index: number,
  overrides: Partial<NotificationSummaryDto> = {},
): NotificationSummaryDto {
  return {
    id: `notification-${String(index)}`,
    agentId: "alpha",
    sessionId: `session-${String(index)}`,
    sessionTitle: `会话 ${String(index)}`,
    title: `通知 ${String(index)}`,
    preview: `正文摘要 ${String(index)}`,
    occurredAt: `2026-09-19T${String(index % 24).padStart(2, "0")}:00:00Z`,
    deliveryStates: ["Sent"],
    ...overrides,
  };
}

export function deliveryFixture(
  id: string,
  overrides: Partial<DeliveryDto> = {},
): DeliveryDto {
  return {
    id,
    notificationId: "notification-1",
    channelId: "future-channel",
    accountId: "account-a",
    state: "Sent",
    externalMessageId: "external-message-1",
    error: null,
    retryable: false,
    updatedAt: "2026-09-19T00:00:00Z",
    ...overrides,
  };
}

export function notificationDetailFixture(
  notification: NotificationSummaryDto,
  overrides: Partial<NotificationDetailDto> = {},
): NotificationDetailDto {
  return {
    notification,
    body: longChineseText,
    metadata: {},
    deliveries: [deliveryFixture("delivery-1", { notificationId: notification.id })],
    routeExists: true,
    ...overrides,
  };
}

export function diagnosticItemFixture(
  code: string,
  overrides: Partial<DiagnosticItemDto> = {},
): DiagnosticItemDto {
  return {
    code,
    level: "Normal",
    message: "检查通过",
    checkedAt: "2026-09-19T00:00:00Z",
    action: null,
    ...overrides,
  };
}

export function diagnosticsFixture(
  overrides: Partial<DiagnosticsDto> = {},
): DiagnosticsDto {
  return {
    generatedAt: "2026-09-19T00:00:00Z",
    runtime: {
      appVersion: "2.0.0-dev.0",
      platform: "windows",
      state: "Running",
      paused: false,
    },
    storage: {
      notificationCount: 0,
      deliveryCount: 0,
      pendingOutboxCount: 0,
      recentError: null,
    },
    components: [],
    items: [],
    migration: legacyMigrationFixture(),
    ...overrides,
  };
}

export function settingsFixture(overrides: Partial<SettingsDto> = {}): SettingsDto {
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
export function legacyMigrationFixture(
  overrides: Partial<LegacyMigrationDto> = {},
): LegacyMigrationDto {
  return {
    state: "NotConfigured",
    sourceDetected: false,
    reportFile: null,
    report: null,
    error: null,
    ...overrides,
  };
}
