import type {
  AdvanceOrcTaskPayload,
  AgentDto,
  BeginChannelLoginPayload,
  BeginChannelLoginResultDto,
  BusinessCommand as GeneratedBusinessCommand,
  ChannelAccountDto,
  ChannelAccountIdPayload,
  ChannelListDto,
  CommandError,
  CreateOrcTaskPayload,
  CurrentOrcWorkflowDto,
  DeliveryDto,
  DeliveryIdPayload,
  DiagnosticsDto,
  EmptyPayload,
  HostEvent as GeneratedHostEvent,
  InstallUpdatePayload,
  InstallUpdateResultDto,
  LegacyMigrationDto,
  LoginSessionDto,
  MarkBlockedOrcTaskPayload,
  MutationAcceptedDto,
  NotificationDetailDto,
  NotificationFilterPayload,
  NotificationIdPayload,
  NotificationListDto,
  OpencodeModelDto,
  OpencodeProjectDto,
  OrcTaskDto,
  OrcTaskIdPayload,
  OrcTemplateDto,
  RuntimeSnapshotDto,
  RuntimeSummaryDto,
  SaveOrcTemplateConfigPayload,
  SendTestNotificationPayload,
  SetRuntimePausedPayload,
  SettingsDto,
  SubmitChannelLoginCodePayload,
  TestNotificationResultDto,
  UpdateAgentConfigPayload,
  UpdateOrcTaskPayload,
  UpdateStatusDto,
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
  retry_legacy_migration: EmptyPayload;
  get_settings: EmptyPayload;
  update_settings: SettingsDto;
  set_runtime_paused: SetRuntimePausedPayload;
  quit_app: EmptyPayload;
  get_update_status: EmptyPayload;
  install_update: InstallUpdatePayload;
  create_orc_task: CreateOrcTaskPayload;
  list_orc_tasks: EmptyPayload;
  advance_orc_task: AdvanceOrcTaskPayload;
  mark_blocked_orc_task: MarkBlockedOrcTaskPayload;
  recover_blocked_orc_task: OrcTaskIdPayload;
  start_orc_task: OrcTaskIdPayload;
  update_orc_task: UpdateOrcTaskPayload;
  delete_orc_task: OrcTaskIdPayload;
  get_current_orc_workflow: EmptyPayload;
  list_orc_templates: EmptyPayload;
  save_orc_template_config: SaveOrcTemplateConfigPayload;
  list_opencode_projects: EmptyPayload;
  list_opencode_models: EmptyPayload;
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
  retry_legacy_migration: LegacyMigrationDto;
  get_settings: SettingsDto;
  update_settings: SettingsDto;
  set_runtime_paused: RuntimeSummaryDto;
  quit_app: MutationAcceptedDto;
  get_update_status: UpdateStatusDto;
  install_update: InstallUpdateResultDto;
  create_orc_task: OrcTaskDto;
  list_orc_tasks: OrcTaskDto[];
  advance_orc_task: OrcTaskDto;
  mark_blocked_orc_task: OrcTaskDto;
  recover_blocked_orc_task: OrcTaskDto;
  start_orc_task: OrcTaskDto;
  update_orc_task: OrcTaskDto;
  delete_orc_task: MutationAcceptedDto;
  get_current_orc_workflow: CurrentOrcWorkflowDto;
  list_orc_templates: OrcTemplateDto[];
  save_orc_template_config: OrcTemplateDto[];
  list_opencode_projects: OpencodeProjectDto[];
  list_opencode_models: OpencodeModelDto[];
}

export interface EventPayloadMap {
  "snapshot.changed": { reason: string };
  "delivery.changed": {
    deliveryId: string;
    notificationId?: string | null;
    state?: DeliveryDto["state"] | null;
  };
  "channel.login.changed": {
    accountId?: string | null;
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
