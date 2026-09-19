import { useEffect, useState } from "react";

import type { HostBridge } from "../../bridge";
import type { LoginSessionDto } from "../../bridge/types";
import {
  useBeginChannelLoginMutation,
  useSubmitChannelLoginCodeMutation,
} from "../../data/mutations";

export interface ActiveChannelLogin {
  channelId: string;
  accountKey: string;
  session: LoginSessionDto;
}

function accountKey(): string {
  if (typeof crypto !== "undefined" && "randomUUID" in crypto) {
    return `account-${crypto.randomUUID()}`;
  }
  return `account-${Date.now()}-${Math.random().toString(16).slice(2)}`;
}

export function useChannelLogin(bridge: HostBridge) {
  const [activeByChannel, setActiveByChannel] = useState<
    Record<string, ActiveChannelLogin>
  >({});
  const [lastError, setLastError] = useState<{
    channelId: string;
    error: unknown;
  } | null>(null);
  const beginMutation = useBeginChannelLoginMutation(bridge);
  const submitMutation = useSubmitChannelLoginCodeMutation(bridge);

  useEffect(
    () =>
      bridge.subscribe("channel.login.changed", (event) => {
        setActiveByChannel((current) => {
          const entries = Object.entries(current);
          const sessionMatch = event.sessionId
            ? entries.find(([, login]) => login.session.id === event.sessionId)
            : undefined;
          const accountMatch = entries.find(
            ([, login]) => login.session.accountId === event.accountId,
          );
          const pendingEntries = entries.filter(
            ([, login]) => login.session.accountId === null,
          );
          // 会话 ID 缺失时，仅在唯一待绑定会话存在时才承接事件，避免多账号串状态。
          const pendingMatch =
            !event.sessionId && pendingEntries.length === 1
              ? pendingEntries[0]
              : undefined;
          const match: [string, ActiveChannelLogin] | undefined =
            sessionMatch ?? accountMatch ?? pendingMatch;

          if (!match) {
            return current;
          }

          const [channelId, active] = match;
          return {
            ...current,
            [channelId]: {
              ...active,
              session: {
                ...active.session,
                accountId: event.accountId || active.session.accountId,
                state: event.state,
                message:
                  event.message == null
                    ? active.session.message
                    : event.message,
              },
            },
          };
        });
      }),
    [bridge],
  );

  const begin = async (channelId: string, requestedAccountKey?: string) => {
    setLastError(null);
    const nextAccountKey = requestedAccountKey?.trim() || accountKey();

    try {
      const result = await beginMutation.mutateAsync({
        channelId,
        accountKey: nextAccountKey,
      });
      setActiveByChannel((current) => ({
        ...current,
        [channelId]: {
          channelId: result.channelId,
          accountKey: nextAccountKey,
          session: result.session,
        },
      }));
    } catch (error) {
      setLastError({ channelId, error });
    }
  };

  const submitCode = async (channelId: string, code: string) => {
    const current = activeByChannel[channelId];
    if (!current) {
      return;
    }

    setLastError(null);
    try {
      const session = await submitMutation.mutateAsync({
        sessionId: current.session.id,
        code,
      });
      setActiveByChannel((latest) => {
        const latestLogin = latest[channelId];
        if (!latestLogin || latestLogin.session.id !== current.session.id) {
          return latest;
        }
        return {
          ...latest,
          [channelId]: { ...latestLogin, session },
        };
      });
    } catch (error) {
      setLastError({ channelId, error });
    }
  };

  const refresh = async (channelId: string) => {
    const current = activeByChannel[channelId];
    if (!current) {
      return;
    }
    await begin(channelId, current.accountKey);
  };

  return {
    activeFor: (channelId: string | null) =>
      channelId ? (activeByChannel[channelId] ?? null) : null,
    errorFor: (channelId: string | null) =>
      lastError && lastError.channelId === channelId ? lastError.error : null,
    isPreparingFor: (channelId: string | null) =>
      beginMutation.isPending &&
      channelId !== null &&
      beginMutation.variables?.channelId === channelId,
    isSubmittingFor: (channelId: string | null) =>
      submitMutation.isPending &&
      channelId !== null &&
      activeByChannel[channelId]?.session.id ===
        submitMutation.variables?.sessionId,
    begin,
    refresh,
    submitCode,
  };
}
