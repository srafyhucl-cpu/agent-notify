import type {
  AgentDto,
  BeginChannelLoginPayload,
  BeginChannelLoginResultDto,
  BusinessCommand,
  ChannelAccountDto,
  ChannelDto,
  CommandError,
  DeliveryDto,
  DiagnosticsDto,
  EmptyPayload,
  HostEvent,
  LoginSessionDto,
  MutationAcceptedDto,
  NotificationDetailDto,
  NotificationListDto,
  NotificationSummaryDto,
  RuntimeSnapshotDto,
  RuntimeSummaryDto,
  SettingsDto,
} from "./types";
import type {
  CommandPayloadMap,
  CommandResultMap,
  EventPayloadMap,
  HostBridge,
} from "./hostBridge";

export interface BridgeInvocation {
  command: BusinessCommand;
  payload: unknown;
}

export interface MockHostBridgeOptions {
  agents?: AgentDto[];
  channels?: ChannelDto[];
  snapshot?: RuntimeSnapshotDto;
  notifications?: NotificationSummaryDto[];
  notificationDetails?: Record<string, NotificationDetailDto>;
  settings?: SettingsDto;
  diagnostics?: DiagnosticsDto;
  loginSession?: LoginSessionDto;
  errors?: Partial<Record<BusinessCommand, CommandError>>;
  delays?: Partial<Record<BusinessCommand, number>>;
}

export interface MockHostBridge extends HostBridge {
  calls(command?: BusinessCommand): BridgeInvocation[];
  emit<TEvent extends HostEvent>(
    event: TEvent,
    payload: EventPayloadMap[TEvent],
  ): void;
}

const MOCK_QR_PAYLOAD =
  "data:image/gif;base64,R0lGODlhAQABAIAAAAAAAP///ywAAAAAAQABAAACAUwAOw==";

function runtimeSummary(paused = false): RuntimeSummaryDto {
  return {
    appVersion: "2.0.0-dev.0",
    platform: "windows",
    state: paused ? "Paused" : "Running",
    paused,
  };
}

function defaultSnapshot(): RuntimeSnapshotDto {
  return {
    runtime: runtimeSummary(),
    overview: {
      storage: {
        notificationCount: 0,
        deliveryCount: 0,
        pendingOutboxCount: 0,
        recentError: null,
      },
      agents: [],
      channels: [],
      recentDeliveries: [],
    },
    components: [],
    diagnostics: [],
  };
}

function defaultSettings(): SettingsDto {
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
  };
}

function defaultChannel(): ChannelDto {
  return {
    id: "test-channel",
    displayName: "测试渠道",
    configSchema: {
      type: "object",
      properties: {},
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
  };
}

function defaultAgent(): AgentDto {
  return {
    id: "test-agent",
    displayName: "测试 Agent",
    description: "用于桌面 UI 测试的假适配器",
    configSchema: {
      type: "object",
      properties: {},
    },
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

function createLoginSession(
  payload: BeginChannelLoginPayload,
  session?: LoginSessionDto,
): LoginSessionDto {
  if (session) {
    return session;
  }
  return {
    id: "login-session-1",
    accountId: null,
    accountKey: payload.accountKey,
    state: "QrReady",
    qrPayload: MOCK_QR_PAYLOAD,
    createdAt: "2026-09-19T00:00:00Z",
    message: "请使用渠道客户端扫码",
    error: null,
  };
}

function accepted(id?: string): MutationAcceptedDto {
  return {
    accepted: true,
    id: id ?? null,
  };
}

function deliveryFixture(id: string, accountId: string): DeliveryDto {
  return {
    id,
    notificationId: "notification-1",
    channelId: "test-channel",
    accountId,
    state: "Sent",
    externalMessageId: "external-message-1",
    error: null,
    updatedAt: "2026-09-19T00:00:00Z",
  };
}

function replaceAccount(
  channels: ChannelDto[],
  accountId: string,
  enabled: boolean,
): ChannelAccountDto {
  for (const channel of channels) {
    const account = channel.accounts.find((candidate) => candidate.id === accountId);
    if (account) {
      return { ...account, enabled };
    }
  }
  return {
    id: accountId,
    channelId: "test-channel",
    displayName: accountId,
    enabled,
    config: {},
    health: {
      available: enabled,
      stale: false,
      detail: null,
    },
    lastInboundAt: null,
    lastDeliveryAt: null,
  };
}

export function createMockHostBridge(
  options: MockHostBridgeOptions = {},
): MockHostBridge {
  const invocations: BridgeInvocation[] = [];
  const listeners = new Map<HostEvent, Set<(payload: unknown) => void>>();
  const channels = options.channels ?? [defaultChannel()];
  const agents = options.agents ?? [];

  async function applyDelay(command: BusinessCommand) {
    const delay = options.delays?.[command] ?? 0;
    if (delay <= 0) {
      return;
    }
    await new Promise((resolve) => window.setTimeout(resolve, delay));
  }

  async function invoke<TCommand extends BusinessCommand>(
    command: TCommand,
    payload: CommandPayloadMap[TCommand],
  ): Promise<CommandResultMap[TCommand]> {
    invocations.push({ command, payload });
    await applyDelay(command);

    const error = options.errors?.[command];
    if (error) {
      throw { ...error };
    }

    let result: unknown;
    switch (command) {
      case "get_snapshot":
        result = options.snapshot ?? defaultSnapshot();
        break;
      case "list_agents":
        result = agents;
        break;
      case "update_agent_config": {
        const update = payload as CommandPayloadMap["update_agent_config"];
        const current =
          agents.find((agent) => agent.id === update.agentId) ?? defaultAgent();
        result = {
          ...current,
          enabled: update.enabled ?? current.enabled,
          config: update.config ?? current.config,
        } satisfies AgentDto;
        break;
      }
      case "list_channel_accounts":
        result = { channels };
        break;
      case "begin_channel_login": {
        const login = payload as BeginChannelLoginPayload;
        result = {
          channelId: login.channelId,
          session: createLoginSession(login, options.loginSession),
        } satisfies BeginChannelLoginResultDto;
        break;
      }
      case "submit_channel_login_code":
        result = {
          ...createLoginSession(
            { channelId: "test-channel", accountKey: "account-key-1" },
            options.loginSession,
          ),
          state: "WaitingFirstInbound",
          qrPayload: null,
          message: "验证码已提交，等待首条入站消息",
        } satisfies LoginSessionDto;
        break;
      case "logout_channel_account": {
        const account = payload as CommandPayloadMap["logout_channel_account"];
        result = accepted(account.accountId);
        break;
      }
      case "enable_channel_account": {
        const account = payload as CommandPayloadMap["enable_channel_account"];
        result = replaceAccount(channels, account.accountId, true);
        break;
      }
      case "disable_channel_account": {
        const account = payload as CommandPayloadMap["disable_channel_account"];
        result = replaceAccount(channels, account.accountId, false);
        break;
      }
      case "send_test_notification": {
        const test = payload as CommandPayloadMap["send_test_notification"];
        result = {
          accepted: true,
          delivery: deliveryFixture("delivery-test-1", test.accountId),
        };
        break;
      }
      case "list_notifications":
        result = {
          items: options.notifications ?? [],
          total: options.notifications?.length ?? 0,
          nextCursor: null,
        } satisfies NotificationListDto;
        break;
      case "get_notification_detail": {
        const detail = payload as CommandPayloadMap["get_notification_detail"];
        result = options.notificationDetails?.[detail.notificationId] ?? {
          notification: {
            id: detail.notificationId,
            agentId: "test-agent",
            sessionId: null,
            sessionTitle: null,
            title: "测试通知",
            preview: "测试通知正文",
            occurredAt: "2026-09-19T00:00:00Z",
            deliveryStates: ["Sent"],
          },
          body: "测试通知正文",
          metadata: {},
          deliveries: [deliveryFixture("delivery-test-1", "account-1")],
          routeExists: true,
        } satisfies NotificationDetailDto;
        break;
      }
      case "retry_delivery": {
        const retry = payload as CommandPayloadMap["retry_delivery"];
        result = deliveryFixture(retry.deliveryId, "account-1");
        break;
      }
      case "get_diagnostics":
        result =
          options.diagnostics ??
          ({
            generatedAt: "2026-09-19T00:00:00Z",
            runtime: runtimeSummary(),
            storage: {
              notificationCount: 0,
              deliveryCount: 0,
              pendingOutboxCount: 0,
              recentError: null,
            },
            components: [],
            items: [],
          } satisfies DiagnosticsDto);
        break;
      case "get_settings":
        result = options.settings ?? defaultSettings();
        break;
      case "update_settings":
        result = payload as SettingsDto;
        break;
      case "set_runtime_paused": {
        const pause = payload as CommandPayloadMap["set_runtime_paused"];
        result = runtimeSummary(pause.paused);
        break;
      }
      case "quit_app":
        result = accepted();
        break;
      default: {
        const neverCommand: never = command;
        throw new Error(`未处理命令: ${String(neverCommand)}`);
      }
    }

    return result as CommandResultMap[TCommand];
  }

  return {
    invoke,
    subscribe(event, handler) {
      const eventListeners = listeners.get(event) ?? new Set();
      const untypedHandler = handler as (payload: unknown) => void;
      eventListeners.add(untypedHandler);
      listeners.set(event, eventListeners);
      return () => {
        eventListeners.delete(untypedHandler);
      };
    },
    calls(command) {
      if (!command) {
        return [...invocations];
      }
      return invocations.filter((invocation) => invocation.command === command);
    },
    emit(event, payload) {
      for (const handler of listeners.get(event) ?? []) {
        handler(payload);
      }
    },
  };
}
