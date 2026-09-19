import {
  AlertTriangle,
  Plus,
  RadioTower,
  Send,
  X,
} from "lucide-react";
import {
  useEffect,
  useMemo,
  useState,
  type FormEvent,
} from "react";
import { useQuery } from "@tanstack/react-query";

import type { HostBridge } from "../../bridge";
import type { ChannelAccountDto } from "../../bridge/types";
import { EmptyState } from "../../components/EmptyState";
import { InlineError } from "../../components/InlineError";
import { LoadingRows } from "../../components/LoadingRows";
import { toUserError } from "../../data/errors";
import {
  useDisableChannelAccountMutation,
  useEnableChannelAccountMutation,
  useLogoutChannelAccountMutation,
  useSendTestNotificationMutation,
} from "../../data/mutations";
import { queryKeys } from "../../data/queryKeys";
import { ChannelAccountDetail } from "./ChannelAccountDetail";
import {
  ChannelAccountList,
  type ChannelAccountEntry,
} from "./ChannelAccountList";
import { ChannelLoginDialog } from "./ChannelLoginDialog";
import { useChannelLogin } from "./useChannelLogin";
import "../../styles/channels.css";

function accountsFromChannels(
  channels: Array<{
    id: string;
    displayName: string;
    configSchema: unknown;
    capabilities: {
      sendText: boolean;
      receive: boolean;
      replyRouting: boolean;
      editMessage: boolean;
      attachments: boolean;
      markdown: boolean;
      maxTextBytes: number | null;
      inboundModes: string[];
    };
    accounts: ChannelAccountDto[];
  }>,
): ChannelAccountEntry[] {
  return channels.flatMap((channel) =>
    channel.accounts.map((account) => ({ channel, account })),
  );
}

function utf8ByteLength(value: string): number {
  return new TextEncoder().encode(value).length;
}

export interface ChannelsPageProps {
  bridge: HostBridge;
}

export function ChannelsPage({ bridge }: ChannelsPageProps) {
  const channelsQuery = useQuery({
    queryKey: queryKeys.channels(),
    queryFn: () => bridge.invoke("list_channel_accounts", {}),
    staleTime: 0,
  });
  const [selectedAccountId, setSelectedAccountId] = useState<string | null>(null);
  const selectedAccountQuery = useQuery({
    queryKey: [...queryKeys.channels(), "account", selectedAccountId],
    queryFn: () => bridge.invoke("list_channel_accounts", {}),
    enabled: selectedAccountId !== null,
    staleTime: 0,
    refetchOnMount: "always",
    placeholderData: undefined,
  });
  const enableMutation = useEnableChannelAccountMutation(bridge);
  const disableMutation = useDisableChannelAccountMutation(bridge);
  const logoutMutation = useLogoutChannelAccountMutation(bridge);
  const sendMutation = useSendTestNotificationMutation(bridge);
  const login = useChannelLogin(bridge);
  const [pendingAccountId, setPendingAccountId] = useState<string | null>(null);
  const [actionError, setActionError] = useState<unknown>(null);
  const [logoutTarget, setLogoutTarget] =
    useState<ChannelAccountDto | null>(null);
  const [loginOpen, setLoginOpen] = useState(false);
  const [loginChannelId, setLoginChannelId] = useState<string | null>(null);
  const [sendAccountId, setSendAccountId] = useState("");
  const [sendTitle, setSendTitle] = useState("");
  const [sendBody, setSendBody] = useState("");
  const [sendError, setSendError] = useState<unknown>(null);

  const channels = channelsQuery.data?.channels ?? [];
  const entries = useMemo(
    () => accountsFromChannels(channels),
    [channels],
  );
  const selectedEntry =
    selectedAccountQuery.data?.channels
      .flatMap((channel) =>
        channel.accounts.map((account) => ({ channel, account })),
      )
      .find((entry) => entry.account.id === selectedAccountId) ?? null;
  const loginChannel =
    channels.find((channel) => channel.id === loginChannelId) ??
    channels[0] ??
    null;
  const sendEntries = entries.filter(
    ({ channel }) => channel.capabilities.sendText,
  );
  const selectedSendEntry =
    sendEntries.find(({ account }) => account.id === sendAccountId) ?? null;
  const maxTextBytes = selectedSendEntry?.channel.capabilities.maxTextBytes;
  const bodyTooLarge =
    maxTextBytes !== null &&
    maxTextBytes !== undefined &&
    utf8ByteLength(sendBody) > maxTextBytes;
  const loadError = channelsQuery.error ? toUserError(channelsQuery.error) : null;
  const detailError = selectedAccountQuery.error
    ? toUserError(selectedAccountQuery.error)
    : null;
  const actionUserError = actionError ? toUserError(actionError) : null;
  const sendUserError = sendError ? toUserError(sendError) : null;

  useEffect(() => {
    if (
      selectedAccountId &&
      !entries.some((entry) => entry.account.id === selectedAccountId)
    ) {
      setSelectedAccountId(null);
      return;
    }
    if (!selectedAccountId && entries[0]) {
      setSelectedAccountId(entries[0].account.id);
    }
  }, [entries, selectedAccountId]);

  useEffect(() => {
    if (
      sendAccountId &&
      !sendEntries.some((entry) => entry.account.id === sendAccountId)
    ) {
      setSendAccountId("");
    }
  }, [sendAccountId, sendEntries]);

  const toggleAccount = async (
    account: ChannelAccountDto,
    enabled: boolean,
  ) => {
    setActionError(null);
    setPendingAccountId(account.id);
    try {
      const mutation = enabled ? enableMutation : disableMutation;
      await mutation.mutateAsync({ accountId: account.id });
    } catch (error) {
      setActionError(error);
    } finally {
      setPendingAccountId(null);
    }
  };

  const openLogin = (channelId: string) => {
    setLoginChannelId(channelId);
    setLoginOpen(true);
    if (!login.activeFor(channelId)) {
      void login.begin(channelId);
    }
  };

  const confirmLogout = async () => {
    if (!logoutTarget) {
      return;
    }

    setActionError(null);
    setPendingAccountId(logoutTarget.id);
    try {
      await logoutMutation.mutateAsync({ accountId: logoutTarget.id });
      setLogoutTarget(null);
    } catch (error) {
      setActionError(error);
    } finally {
      setPendingAccountId(null);
    }
  };

  const submitTestNotification = async (event: FormEvent) => {
    event.preventDefault();
    setSendError(null);

    if (!sendAccountId) {
      setSendError({
        code: "test_account_required",
        message: "请选择一个具体账号后再发送测试通知。",
        retryable: false,
      });
      return;
    }
    if (bodyTooLarge) {
      setSendError({
        code: "test_body_too_large",
        message: `正文超过 ${String(maxTextBytes)} 字节，请缩短后再发送。`,
        retryable: false,
      });
      return;
    }

    try {
      await sendMutation.mutateAsync({
        accountId: sendAccountId,
        title: sendTitle.trim(),
        body: sendBody,
      });
      setSendAccountId("");
      setSendTitle("");
      setSendBody("");
    } catch (error) {
      setSendError(error);
    }
  };

  return (
    <section className="workbench-page" aria-labelledby="page-title-channels">
      <header className="workbench-page-header">
        <div>
          <h1 className="workbench-page-title" id="page-title-channels">
            Channels
          </h1>
          <p className="page-summary">
            按 descriptor 管理多个渠道账号、登录状态和测试发送。
          </p>
        </div>
        <span
          className="page-count"
          aria-label={`共 ${String(entries.length)} 个渠道账号`}
        >
          <RadioTower aria-hidden="true" size={16} />
          {entries.length} 个账号
        </span>
      </header>

      <div className="workbench-page-content channels-page-content">
        {loadError ? (
          <InlineError
            title="无法读取渠道账号"
            message={loadError.message}
            action={
              <button
                className="button button-secondary"
                type="button"
                onClick={() => void channelsQuery.refetch()}
              >
                重新检查
              </button>
            }
          />
        ) : null}

        {actionUserError ? (
          <InlineError
            title={actionUserError.title}
            message={actionUserError.message}
          />
        ) : null}

        {channelsQuery.isPending && !channelsQuery.data ? (
          <LoadingRows aria-label="正在加载渠道账号列表" />
        ) : null}

        {!channelsQuery.isPending && channels.length === 0 && !loadError ? (
          <EmptyState
            title="暂无渠道"
            description="宿主尚未注册渠道 descriptor，当前无法添加账号。"
          />
        ) : null}

        {channels.map((channel) => {
          const channelEntries = entries.filter(
            (entry) => entry.channel.id === channel.id,
          );
          return (
            <section
              className="channel-band"
              aria-labelledby={`channel-${channel.id}`}
              key={channel.id}
            >
              <header className="channel-band-header">
                <div>
                  <h2 id={`channel-${channel.id}`}>{channel.displayName}</h2>
                  <p className="section-description">
                    {channel.capabilities.sendText ? "支持文本发送" : "只接收消息"}
                    {" · "}
                    {channel.capabilities.replyRouting ? "支持回复路由" : "不支持回复路由"}
                  </p>
                </div>
                <button
                  className="button button-secondary"
                  type="button"
                  onClick={() => openLogin(channel.id)}
                >
                  <Plus aria-hidden="true" size={15} />
                  添加渠道账号
                </button>
              </header>

              {channelEntries.length > 0 ? (
                <ChannelAccountList
                  entries={channelEntries}
                  selectedAccountId={selectedAccountId}
                  pendingAccountId={pendingAccountId}
                  onSelect={setSelectedAccountId}
                  onToggle={(account, enabled) =>
                    void toggleAccount(account, enabled)
                  }
                />
              ) : (
                <p className="section-empty">
                  此渠道还没有账号，请添加账号并完成登录。
                </p>
              )}
            </section>
          );
        })}

        {detailError ? (
          <InlineError
            title="无法读取账号详情"
            message={detailError.message}
            action={
              <button
                className="button button-secondary"
                type="button"
                onClick={() => void selectedAccountQuery.refetch()}
              >
                重新加载
              </button>
            }
          />
        ) : null}

        {selectedAccountId && !selectedEntry && !detailError ? (
          <LoadingRows aria-label="正在加载账号详情" rows={2} />
        ) : null}

        {selectedEntry ? (
          <ChannelAccountDetail
            key={selectedEntry.account.id}
            channel={selectedEntry.channel}
            account={selectedEntry.account}
            pending={pendingAccountId === selectedEntry.account.id}
            onToggle={(account, enabled) =>
              void toggleAccount(account, enabled)
            }
            onLogout={setLogoutTarget}
          />
        ) : null}

        {sendEntries.length > 0 ? (
          <form
            className="test-notification-form"
            aria-label="测试发送"
            onSubmit={(event) => void submitTestNotification(event)}
          >
            <div className="test-notification-heading">
              <Send aria-hidden="true" size={18} />
              <div>
                <h2>测试发送</h2>
                <p>必须选择具体账号，不会使用隐式默认账号。</p>
              </div>
            </div>

            <div className="test-notification-fields">
              <label>
                <span>测试发送账号</span>
                <select
                  value={sendAccountId}
                  disabled={sendMutation.isPending}
                  onChange={(event) => setSendAccountId(event.currentTarget.value)}
                >
                  <option value="">请选择具体账号</option>
                  {sendEntries.map(({ channel, account }) => (
                    <option
                      value={account.id}
                      disabled={!account.enabled}
                      key={account.id}
                    >
                      {channel.displayName} · {account.displayName} ({account.id})
                    </option>
                  ))}
                </select>
              </label>

              <label>
                <span>标题</span>
                <input
                  type="text"
                  value={sendTitle}
                  disabled={sendMutation.isPending}
                  onChange={(event) => setSendTitle(event.currentTarget.value)}
                />
              </label>

              <label className="test-body-field">
                <span>正文</span>
                <textarea
                  rows={3}
                  value={sendBody}
                  disabled={sendMutation.isPending}
                  aria-invalid={bodyTooLarge}
                  onChange={(event) => setSendBody(event.currentTarget.value)}
                />
              </label>
            </div>

            <div className="test-notification-footer">
              <span className={bodyTooLarge ? "field-error" : "field-hint"}>
                {bodyTooLarge
                  ? `正文超过 ${String(maxTextBytes)} 字节`
                  : maxTextBytes === null || maxTextBytes === undefined
                    ? "当前渠道未声明正文长度限制"
                    : `${String(utf8ByteLength(sendBody))} / ${String(maxTextBytes)} 字节`}
              </span>
              <button
                className="button"
                type="submit"
                disabled={
                  sendMutation.isPending ||
                  !sendAccountId ||
                  sendTitle.trim() === "" ||
                  sendBody.trim() === "" ||
                  bodyTooLarge
                }
              >
                <Send aria-hidden="true" size={15} />
                {sendMutation.isPending ? "正在发送" : "发送测试通知"}
              </button>
            </div>

            {sendUserError ? (
              <InlineError
                title={sendUserError.title}
                message={sendUserError.message}
              />
            ) : null}
          </form>
        ) : null}
      </div>

      <ChannelLoginDialog
        open={loginOpen}
        channel={loginChannel}
        active={login.activeFor(loginChannel?.id ?? null)}
        isPreparing={login.isPreparingFor(loginChannel?.id ?? null)}
        isSubmitting={login.isSubmittingFor(loginChannel?.id ?? null)}
        actionError={login.errorFor(loginChannel?.id ?? null)}
        onBegin={(channelId) => void login.begin(channelId)}
        onRefresh={() =>
          loginChannel ? void login.refresh(loginChannel.id) : undefined
        }
        onSubmitCode={(code) =>
          loginChannel
            ? void login.submitCode(loginChannel.id, code)
            : undefined
        }
        onClose={() => setLoginOpen(false)}
      />

      {logoutTarget ? (
        <div className="dialog-overlay" role="presentation">
          <section
            className="confirm-dialog"
            role="alertdialog"
            aria-modal="true"
            aria-labelledby="logout-title"
          >
            <AlertTriangle aria-hidden="true" size={24} />
            <div>
              <h2 id="logout-title">退出 {logoutTarget.displayName}</h2>
              <p>
                退出后，该账号相关的路由和游标会失效，历史保留。
                下次使用需要重新登录。
              </p>
            </div>
            <div className="confirm-dialog-actions">
              <button
                className="button button-secondary"
                type="button"
                disabled={pendingAccountId === logoutTarget.id}
                onClick={() => setLogoutTarget(null)}
              >
                <X aria-hidden="true" size={15} />
                取消
              </button>
              <button
                className="button button-danger"
                type="button"
                disabled={pendingAccountId === logoutTarget.id}
                onClick={() => void confirmLogout()}
              >
                确认退出
              </button>
            </div>
          </section>
        </div>
      ) : null}
    </section>
  );
}
