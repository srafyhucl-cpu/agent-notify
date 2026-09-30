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
  OpencodeModelDto,
  OpencodeProjectDto,
  OrcTaskDto,
  OrcTemplateDto,
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

/** 编排任务样本（P1 集群页，§3.2 TASK 的脱敏视图）。 */
export function orcTaskFixture(
  id: string,
  overrides: Partial<OrcTaskDto> = {},
): OrcTaskDto {
  const goal = overrides.goal ?? `集群任务 ${id}`;
  return {
    id,
    workflowId: "workflow-preset",
    state: "working",
    currentStep: 2,
    started: true,
    workflow: {
      id: "workflow-preset",
      name: "需求→判断→规划→实施",
      steps: [
        {
          order: 1,
          role: "orchestrator",
          agentHint: "codex",
          model: null,
          variant: null,
          humanGate: false,
        },
        {
          order: 2,
          role: "planner",
          agentHint: "opencode",
          model: "anthropic/claude-sonnet-4-5",
          variant: null,
          humanGate: false,
        },
        {
          order: 3,
          role: "executor",
          agentHint: "commandcode",
          model: null,
          variant: null,
          humanGate: false,
        },
      ],
    },
    blockedStep: null,
    blockReason: null,
    notifyMode: "final_only",
    goal,
    // 名称与后端同规则：显式优先，缺省按目标前 8 字推导。
    name: overrides.name ?? [...goal].slice(0, 8).join(""),
    round: 1,
    roundInput: null,
    createdAt: "2026-09-29T08:25:02.964Z",
    roundHistory: [],
    workingDir: "D:/Project/agent-notify",
    finalizing: false,
    ...overrides,
  };
}

/** 未配置 Agent 的任务（开始执行预检报错用；首步缺 Agent，且尚未开始）。 */
export function unconfiguredOrcTaskFixture(id: string): OrcTaskDto {
  const task = orcTaskFixture(id, { currentStep: 1, started: false });
  return {
    ...task,
    workflow: {
      ...task.workflow,
      steps: task.workflow.steps.map((step) => ({
        ...step,
        agentHint: null,
        model: null,
      })),
    },
  };
}

/** 编排模板样本（§2 三档内置模板；默认不预填 Agent/模型）。 */
export function orcTemplateFixture(
  id: string,
  overrides: Partial<OrcTemplateDto> = {},
): OrcTemplateDto {
  return {
    id,
    name: `模板 ${id}`,
    steps: [{ order: 1, role: "executor", agent: null, model: null }],
    ...overrides,
  };
}

/** 三档内置模板（与后端 `Workflow::builtin` 同构，节点未配置）。 */
export function orcTemplatesFixture(): OrcTemplateDto[] {
  return [
    {
      id: "template-quickfix",
      name: "快速修复",
      steps: [
        { order: 1, role: "executor", agent: null, model: null },
        { order: 2, role: "reviewer", agent: null, model: null },
      ],
    },
    {
      id: "template-standard",
      name: "标准交付",
      steps: [
        { order: 1, role: "planner", agent: null, model: null },
        { order: 2, role: "executor", agent: null, model: null },
        { order: 3, role: "reviewer", agent: null, model: null },
      ],
    },
    {
      id: "template-full",
      name: "完整评估",
      steps: [
        { order: 1, role: "orchestrator", agent: null, model: null },
        { order: 2, role: "planner", agent: null, model: null },
        { order: 3, role: "executor", agent: null, model: null },
        { order: 4, role: "reviewer", agent: null, model: null },
      ],
    },
  ];
}

/** 已配置 Agent/模型的模板（创建预览展示用）。 */
export function configuredTemplateFixture(): OrcTemplateDto {
  return {
    id: "template-standard",
    name: "标准交付",
    steps: [
      { order: 1, role: "planner", agent: "opencode", model: null },
      {
        order: 2,
        role: "executor",
        agent: "opencode",
        model: "anthropic/claude-sonnet-4-5",
      },
      { order: 3, role: "reviewer", agent: "codex", model: null },
    ],
  };
}

/** OpenCode 项目样本（工作目录下拉）。 */
export function opencodeProjectFixture(
  directory: string,
  overrides: Partial<OpencodeProjectDto> = {},
): OpencodeProjectDto {
  return {
    directory,
    name: null,
    lastActiveAt: 1_760_000_000,
    ...overrides,
  };
}

/** 两个已知项目：按最近活跃倒序（与后端读取顺序一致）。 */
export function opencodeProjectsFixture(): OpencodeProjectDto[] {
  return [
    opencodeProjectFixture("D:/Project/agent-notify", {
      name: "agent-notify",
      lastActiveAt: 1_760_000_000,
    }),
    opencodeProjectFixture("D:/Project/legacy-demo", { lastActiveAt: 1_750_000_000 }),
  ];
}

/** 单条 OpenCode 可用模型样本（`provider/model`、显示名与思考强度候选）。 */
export function opencodeModelFixture(
  providerId: string,
  modelId: string,
  name: string,
  variants: string[] = [],
): OpencodeModelDto {
  return { providerId, modelId, name, variants };
}

/** OpenCode 可用模型样本（模型下拉数据源；跨 provider 同名可从显示名区分）。 */
export function opencodeModelsFixture(): OpencodeModelDto[] {
  return [
    opencodeModelFixture(
      "opencode-go",
      "deepseek-v4.1-flash",
      "DeepSeek V4.1 Flash",
      ["low", "medium", "high", "xhigh", "max"],
    ),
    opencodeModelFixture("opencode-go", "space-bunny-free", "Space Bunny Free", ["none"]),
    opencodeModelFixture("opencode", "mimo-v2.6-flash", "MiMo-V2.6-Flash"),
  ];
}
