import { commands, events } from "./types";
import type {
  BusinessCommand,
  CommandPayloadMap,
  CommandResultMap,
  EventPayloadMap,
  HostBridge,
} from "./hostBridge";

function subscribeToEvent<T>(
  listener: { listen: (handler: (event: { payload: T }) => void) => Promise<() => void> },
  handler: (payload: T) => void,
): () => void {
  let cancelled = false;
  let unlisten: (() => void) | undefined;

  void listener
    .listen((event) => {
      handler(event.payload);
    })
    .then((dispose) => {
      if (cancelled) {
        dispose();
        return;
      }
      unlisten = dispose;
    })
    .catch(() => undefined);

  return () => {
    cancelled = true;
    unlisten?.();
    unlisten = undefined;
  };
}

async function dispatchCommand(
  command: BusinessCommand,
  payload: unknown,
): Promise<unknown> {
  switch (command) {
    case "get_snapshot":
      return commands.getSnapshot(payload as CommandPayloadMap["get_snapshot"]);
    case "list_agents":
      return commands.listAgents(payload as CommandPayloadMap["list_agents"]);
    case "update_agent_config":
      return commands.updateAgentConfig(
        payload as CommandPayloadMap["update_agent_config"],
      );
    case "list_channel_accounts":
      return commands.listChannelAccounts(
        payload as CommandPayloadMap["list_channel_accounts"],
      );
    case "begin_channel_login":
      return commands.beginChannelLogin(
        payload as CommandPayloadMap["begin_channel_login"],
      );
    case "submit_channel_login_code":
      return commands.submitChannelLoginCode(
        payload as CommandPayloadMap["submit_channel_login_code"],
      );
    case "logout_channel_account":
      return commands.logoutChannelAccount(
        payload as CommandPayloadMap["logout_channel_account"],
      );
    case "enable_channel_account":
      return commands.enableChannelAccount(
        payload as CommandPayloadMap["enable_channel_account"],
      );
    case "disable_channel_account":
      return commands.disableChannelAccount(
        payload as CommandPayloadMap["disable_channel_account"],
      );
    case "send_test_notification":
      return commands.sendTestNotification(
        payload as CommandPayloadMap["send_test_notification"],
      );
    case "list_notifications":
      return commands.listNotifications(
        payload as CommandPayloadMap["list_notifications"],
      );
    case "get_notification_detail":
      return commands.getNotificationDetail(
        payload as CommandPayloadMap["get_notification_detail"],
      );
    case "retry_delivery":
      return commands.retryDelivery(payload as CommandPayloadMap["retry_delivery"]);
    case "get_diagnostics":
      return commands.getDiagnostics(payload as CommandPayloadMap["get_diagnostics"]);
    case "get_settings":
      return commands.getSettings(payload as CommandPayloadMap["get_settings"]);
    case "update_settings":
      return commands.updateSettings(payload as CommandPayloadMap["update_settings"]);
    case "set_runtime_paused":
      return commands.setRuntimePaused(
        payload as CommandPayloadMap["set_runtime_paused"],
      );
    case "quit_app":
      return commands.quitApp(payload as CommandPayloadMap["quit_app"]);
    case "get_update_status":
      return commands.getUpdateStatus(
        payload as CommandPayloadMap["get_update_status"],
      );
    default: {
      const neverCommand: never = command;
      throw new Error(`未处理命令: ${String(neverCommand)}`);
    }
  }
}

export function createTauriHostBridge(): HostBridge {
  return {
    async invoke(command, payload) {
      return (await dispatchCommand(command, payload)) as CommandResultMap[typeof command];
    },
    subscribe(event, handler) {
      switch (event) {
        case "snapshot.changed":
          return subscribeToEvent(
            events.snapshotChanged,
            handler as (payload: EventPayloadMap["snapshot.changed"]) => void,
          );
        case "delivery.changed":
          return subscribeToEvent(
            events.deliveryChanged,
            handler as (payload: EventPayloadMap["delivery.changed"]) => void,
          );
        case "channel.login.changed":
          return subscribeToEvent(
            events.channelLoginChanged,
            handler as (payload: EventPayloadMap["channel.login.changed"]) => void,
          );
        default: {
          const neverEvent: never = event;
          void neverEvent;
          return () => undefined;
        }
      }
    },
  };
}
