import type {
  AgentDto,
  BeginChannelLoginPayload,
  BeginChannelLoginResultDto,
  BusinessCommand as GeneratedBusinessCommand,
  ChannelAccountDto,
  ChannelAccountIdPayload,
  ChannelListDto,
  CommandError,
  DeliveryDto,
  DeliveryIdPayload,
  DiagnosticsDto,
  EmptyPayload,
  HostEvent as GeneratedHostEvent,
  LoginSessionDto,
  MutationAcceptedDto,
  NotificationDetailDto,
  NotificationFilterPayload,
  NotificationIdPayload,
  NotificationListDto,
  RuntimeSnapshotDto,
  RuntimeSummaryDto,
  SendTestNotificationPayload,
  SetRuntimePausedPayload,
  SettingsDto,
  SubmitChannelLoginCodePayload,
  TestNotificationResultDto,
  UpdateAgentConfigPayload,
} from "./types";

export type BusinessCommand = GeneratedBusinessCommand;
export type HostEvent = GeneratedHostEvent;

export interface CommandPayloadMap {
  get_snapshot: EmptyPayload;
  list_agents: EmptyPayload;
  update_agent_config: UpdateAgentConfigPayload;
  list_channel_accounts: EmptyPayload;
  begin_channel_login: BeginChannelLoginPayload;
  submit_channel_login_code: SubmitChannelLoginCodePayload;
  logout_channel_account: ChannelAccountIdPayload;
  enable_channel_account: ChannelAccountIdPayload;
  disable_channel_account: ChannelAccountIdPayload;
  send_test_notification: SendTestNotificationPayload;
  list_notifications: NotificationFilterPayload;
  get_notification_detail: NotificationIdPayload;
  retry_delivery: DeliveryIdPayload;
  get_diagnostics: EmptyPayload;
  get_settings: EmptyPayload;
  update_settings: SettingsDto;
  set_runtime_paused: SetRuntimePausedPayload;
  quit_app: EmptyPayload;
}

export interface CommandResultMap {
  get_snapshot: RuntimeSnapshotDto;
  list_agents: AgentDto[];
  update_agent_config: AgentDto;
  list_channel_accounts: ChannelListDto;
  begin_channel_login: BeginChannelLoginResultDto;
  submit_channel_login_code: LoginSessionDto;
  logout_channel_account: MutationAcceptedDto;
  enable_channel_account: ChannelAccountDto;
  disable_channel_account: ChannelAccountDto;
  send_test_notification: TestNotificationResultDto;
  list_notifications: NotificationListDto;
  get_notification_detail: NotificationDetailDto;
  retry_delivery: DeliveryDto;
  get_diagnostics: DiagnosticsDto;
  get_settings: SettingsDto;
  update_settings: SettingsDto;
  set_runtime_paused: RuntimeSummaryDto;
  quit_app: MutationAcceptedDto;
}

export interface EventPayloadMap {
  "snapshot.changed": { reason: string };
  "delivery.changed": {
    deliveryId: string;
    notificationId?: string | null;
    state?: DeliveryDto["state"] | null;
  };
  "channel.login.changed": {
    accountId: string;
    sessionId?: string | null;
    state: LoginSessionDto["state"];
    message?: string | null;
  };
}

export interface HostBridge {
  invoke<TCommand extends BusinessCommand>(
    command: TCommand,
    payload: CommandPayloadMap[TCommand],
  ): Promise<CommandResultMap[TCommand]>;

  subscribe<TEvent extends HostEvent>(
    event: TEvent,
    handler: (payload: EventPayloadMap[TEvent]) => void,
  ): () => void;
}

export type { CommandError };
