import type { NotificationFilterPayload } from "../bridge/types";

export const queryKeys = {
  snapshot: () => ["snapshot"] as const,
  agents: () => ["agents"] as const,
  channels: () => ["channels"] as const,
  deliveries: () => ["deliveries"] as const,
  notifications: (filters?: NotificationFilterPayload) =>
    filters
      ? (["notifications", filters] as const)
      : (["notifications"] as const),
  notificationDetail: (notificationId?: string) =>
    notificationId
      ? (["notifications", "detail", notificationId] as const)
      : (["notifications", "detail"] as const),
  channelLogin: (accountId?: string, sessionId?: string) => {
    if (sessionId) {
      return ["channel-login", accountId ?? null, sessionId] as const;
    }
    if (accountId) {
      return ["channel-login", accountId] as const;
    }
    return ["channel-login"] as const;
  },
  diagnostics: () => ["diagnostics"] as const,
  settings: () => ["settings"] as const,
} as const;
