import {
  useMutation,
  useQueryClient,
  type QueryClient,
} from "@tanstack/react-query";

import type {
  BusinessCommand,
  CommandPayloadMap,
  HostBridge,
} from "../bridge";
import { queryKeys } from "./queryKeys";

async function invalidate(
  queryClient: QueryClient,
  queryKeysToInvalidate: readonly (readonly unknown[])[],
) {
  await Promise.all(
    queryKeysToInvalidate.map((queryKey) =>
      queryClient.invalidateQueries({ queryKey }),
    ),
  );
}

export function useUpdateAgentConfigMutation(bridge: HostBridge) {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: (payload: CommandPayloadMap["update_agent_config"]) =>
      bridge.invoke("update_agent_config", payload),
    onSuccess: () =>
      invalidate(queryClient, [queryKeys.agents(), queryKeys.snapshot()]),
  });
}

export function useBeginChannelLoginMutation(bridge: HostBridge) {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: (payload: CommandPayloadMap["begin_channel_login"]) =>
      bridge.invoke("begin_channel_login", payload),
    onSuccess: () =>
      invalidate(queryClient, [queryKeys.channels(), queryKeys.channelLogin()]),
  });
}

export function useSubmitChannelLoginCodeMutation(bridge: HostBridge) {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: (payload: CommandPayloadMap["submit_channel_login_code"]) =>
      bridge.invoke("submit_channel_login_code", payload),
    onSuccess: (session) => {
      const accountKey = session.accountId
        ? queryKeys.channelLogin(session.accountId)
        : queryKeys.channelLogin();
      return invalidate(queryClient, [queryKeys.channels(), accountKey]);
    },
  });
}

export function useLogoutChannelAccountMutation(bridge: HostBridge) {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: (payload: CommandPayloadMap["logout_channel_account"]) =>
      bridge.invoke("logout_channel_account", payload),
    onSuccess: () =>
      invalidate(queryClient, [queryKeys.channels(), queryKeys.snapshot()]),
  });
}

export function useEnableChannelAccountMutation(bridge: HostBridge) {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: (payload: CommandPayloadMap["enable_channel_account"]) =>
      bridge.invoke("enable_channel_account", payload),
    onSuccess: () =>
      invalidate(queryClient, [queryKeys.channels(), queryKeys.snapshot()]),
  });
}

export function useDisableChannelAccountMutation(bridge: HostBridge) {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: (payload: CommandPayloadMap["disable_channel_account"]) =>
      bridge.invoke("disable_channel_account", payload),
    onSuccess: () =>
      invalidate(queryClient, [queryKeys.channels(), queryKeys.snapshot()]),
  });
}

export function useSendTestNotificationMutation(bridge: HostBridge) {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: (payload: CommandPayloadMap["send_test_notification"]) =>
      bridge.invoke("send_test_notification", payload),
    onSuccess: () =>
      invalidate(queryClient, [
        queryKeys.snapshot(),
        queryKeys.deliveries(),
        queryKeys.notifications(),
      ]),
  });
}

export function useRetryDeliveryMutation(bridge: HostBridge) {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: (payload: CommandPayloadMap["retry_delivery"]) =>
      bridge.invoke("retry_delivery", payload),
    onSuccess: (delivery) =>
      invalidate(queryClient, [
        queryKeys.snapshot(),
        queryKeys.deliveries(),
        queryKeys.notifications(),
        queryKeys.notificationDetail(delivery.notificationId),
      ]),
  });
}

export function useUpdateSettingsMutation(bridge: HostBridge) {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: (payload: CommandPayloadMap["update_settings"]) =>
      bridge.invoke("update_settings", payload),
    onSuccess: () =>
      invalidate(queryClient, [queryKeys.settings(), queryKeys.snapshot()]),
  });
}

export function useSetRuntimePausedMutation(bridge: HostBridge) {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: (payload: CommandPayloadMap["set_runtime_paused"]) =>
      bridge.invoke("set_runtime_paused", payload),
    onSuccess: () => invalidate(queryClient, [queryKeys.snapshot()]),
  });
}

export function useDiagnosticActionMutation<TCommand extends BusinessCommand>(
  bridge: HostBridge,
  command: TCommand,
) {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: (payload: CommandPayloadMap[TCommand]) =>
      bridge.invoke(command, payload),
    onSuccess: () =>
      invalidate(queryClient, [queryKeys.diagnostics(), queryKeys.snapshot()]),
  });
}
