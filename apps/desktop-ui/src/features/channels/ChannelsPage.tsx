import { AlertTriangle, Plus, X } from "lucide-react";
import { useEffect, useMemo, useRef, useState } from "react";
import { useQuery } from "@tanstack/react-query";

import type { HostBridge } from "../../bridge";
import type { ChannelAccountDto } from "../../bridge/types";
import { EmptyState } from "../../components/EmptyState";
import { InlineError } from "../../components/InlineError";
import { Toast } from "../../components/Toast";
import { LoadingRows } from "../../components/LoadingRows";
import {
  EmptyFunnel,
  SectionCard,
} from "../../components/patterns";
import { toUserError } from "../../data/errors";
import { getAccountDisplayName } from "../../data/accountNames";
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

/** 行内「测试发送」的内置内容：正文带账号名，便于在微信里辨认来源。 */
const TEST_NOTIFICATION_TITLE = "测试通知";

/** 结果提示的停留时长：只做确认，不占版面。 */
const SEND_NOTICE_DURATION_MS = 3000;

function testNotificationBody(displayName: string): string {
  return `这是一条来自 Agent-notify 的测试通知，用于验证通道连通性。渠道账号：${displayName}。`;
}

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
    enabled: Boolean(selectedAccountId),
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
  const cancelLogoutRef = useRef<HTMLButtonElement>(null);

  // 与登录对话框一致：Escape 关闭；打开时把焦点移到「取消」（破坏性操作不自动聚焦）。
  useEffect(() => {
    if (!logoutTarget) {
      return;
    }
    cancelLogoutRef.current?.focus();
    const handleKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        setLogoutTarget(null);
      }
    };
    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, [logoutTarget]);
  const [loginOpen, setLoginOpen] = useState(false);
  const [loginChannelId, setLoginChannelId] = useState<string | null>(null);
  const [pendingTestAccountId, setPendingTestAccountId] = useState<string | null>(
    null,
  );
  const [sendNotice, setSendNotice] = useState<string | null>(null);
  const [sendError, setSendError] = useState<unknown>(null);

  const channels = channelsQuery.data?.channels ?? [];
  const firstChannel = channels[0] ?? null;
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
  const loadError = channelsQuery.error ? toUserError(channelsQuery.error) : null;
  const detailError = selectedAccountQuery.error
    ? toUserError(selectedAccountQuery.error)
    : null;
  const actionUserError = actionError ? toUserError(actionError) : null;
  const sendUserError = sendError ? toUserError(sendError) : null;

  const hasInitializedRef = useRef(false);
  useEffect(() => {
    if (!hasInitializedRef.current && entries[0]) {
      setSelectedAccountId(entries[0].account.id);
      hasInitializedRef.current = true;
      return;
    }
    if (
      selectedAccountId &&
      !entries.some((entry) => entry.account.id === selectedAccountId)
    ) {
      setSelectedAccountId(null);
    }
  }, [entries, selectedAccountId]);

  // 结果提示只停留几秒：它是浮层，不占版面，也不该长期挂在窗口上。
  useEffect(() => {
    if (!sendNotice) {
      return;
    }
    const timer = window.setTimeout(
      () => setSendNotice(null),
      SEND_NOTICE_DURATION_MS,
    );
    return () => window.clearTimeout(timer);
  }, [sendNotice]);

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

  /**
   * 行内「测试发送」：标题与正文内置，不需要用户输入；正文必须带账号名，
   * 这样在微信里能一眼看出是哪条账号发来的。
   */
  const sendTestNotification = async (account: ChannelAccountDto) => {
    const displayName = getAccountDisplayName(account);
    setSendError(null);
    setSendNotice(null);
    setPendingTestAccountId(account.id);
    try {
      const result = await sendMutation.mutateAsync({
        accountId: account.id,
        title: TEST_NOTIFICATION_TITLE,
        body: testNotificationBody(displayName),
      });
      const delivery = result.delivery;
      if (delivery?.state === "Sent") {
        setSendNotice("已发送测试消息。");
      } else if (delivery?.state === "Failed") {
        // 命令自身在投递失败时仍返回 accepted=true，这里必须如实反馈失败原因。
        setSendError({
          code: delivery.error?.code ?? "delivery_failed",
          message:
            delivery.error?.message ?? "消息未能送达，请到历史页查看失败原因。",
          retryable: delivery.retryable,
        });
      } else {
        // 仍在投递或后端尚未生成终态：诚实提示已提交，不提前宣称送达。
        setSendNotice("已提交，等待送达。");
      }
    } catch (error) {
      setSendError(error);
    } finally {
      setPendingTestAccountId(null);
    }
  };

  return (
    <section className="workbench-page channels-page">
      <h1 className="visually-hidden">渠道</h1>
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

        {sendNotice ? <Toast message={sendNotice} /> : null}

        {sendUserError ? (
          <InlineError title={sendUserError.title} message={sendUserError.message} />
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

        {!channelsQuery.isPending &&
        !loadError &&
        entries.length === 0 &&
        firstChannel ? (
          <EmptyFunnel
            title="先连接渠道"
            description="连接一个渠道并完成扫码登录后，通知才会开始流动；再发一条测试通知，确认链路真的通了。"
            steps={[
              { title: "选择渠道", description: "在下方选择要接入的渠道。" },
              { title: "扫码登录", description: "用渠道客户端扫码，或提交配对码。" },
              { title: "验证投递", description: "发送一条测试通知，确认能收到。" },
            ]}
            action={
              <button
                className="button"
                type="button"
                onClick={() => openLogin(firstChannel.id)}
              >
                <Plus aria-hidden="true" size={15} />
                连接第一个渠道
              </button>
            }
          />
        ) : null}

        {!channelsQuery.isPending && channels.length === 0 && !loadError ? (
          <EmptyState
            title="暂无渠道"
            description="宿主尚未注册渠道 descriptor，当前无法添加账号。"
          />
        ) : null}

        <div className="channels-container">
          <div className="channels-main">
            {channels.map((channel) => {
              const channelEntries = entries.filter(
                (entry) => entry.channel.id === channel.id,
              );
              const capabilitiesSummary = `${
                channel.capabilities.sendText ? "支持文本发送" : "只接收消息"
              } · ${
                channel.capabilities.replyRouting
                  ? "支持回复路由"
                  : "不支持回复路由"
              }`;
              return (
                <SectionCard
                  key={channel.id}
                  title={channel.displayName}
                  count={channelEntries.length}
                  description={capabilitiesSummary}
                  action={
                    <button
                      className="button button-secondary"
                      type="button"
                      onClick={() => openLogin(channel.id)}
                    >
                      <Plus aria-hidden="true" size={15} />
                      添加渠道账号
                    </button>
                  }
                >
                  {channelEntries.length > 0 ? (
                    <ChannelAccountList
                      entries={channelEntries}
                      selectedAccountId={selectedAccountId}
                      pendingAccountId={pendingAccountId}
                      pendingTestAccountId={pendingTestAccountId}
                      onSelect={(id) => setSelectedAccountId(id || null)}
                      onToggle={(account, enabled) =>
                        void toggleAccount(account, enabled)
                      }
                      onDelete={(account) => setLogoutTarget(account)}
                      onTestSend={(account) =>
                        void sendTestNotification(account)
                      }
                      renderDetail={(entry) => (
                        <div className="channel-detail-inline-wrapper">
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
                          <ChannelAccountDetail
                            key={entry.account.id}
                            channel={entry.channel}
                            account={selectedEntry?.account ?? entry.account}
                            pending={pendingAccountId === entry.account.id}
                            onLogout={setLogoutTarget}
                          />
                        </div>
                      )}
                    />
                  ) : (
                    <p className="section-empty">
                      此渠道还没有账号，请添加账号并完成登录。
                    </p>
                  )}
                </SectionCard>
              );
            })}
          </div>

        </div>
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
        onAccountCreated={(accountId) => {
          const acc = entries.find((e) => e.account.id === accountId)?.account;
          if (acc) {
            void toggleAccount(acc, true);
          }
        }}
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
              <h2 id="logout-title">删除账号 {getAccountDisplayName(logoutTarget)}</h2>
              <p>
                确定要删除该渠道账号吗？删除后，该账号相关的路由和游标会失效，历史保留。
                下次使用需要重新登录。
              </p>
            </div>
            <div className="confirm-dialog-actions">
              <button
                className="button button-secondary"
                type="button"
                ref={cancelLogoutRef}
                disabled={pendingAccountId === logoutTarget.id}
                onClick={() => setLogoutTarget(null)}
              >
                <X aria-hidden="true" size={15} />
                取消
              </button>
              <button
                className="button button-danger"
                type="button"
                aria-label="确认退出"
                disabled={pendingAccountId === logoutTarget.id}
                onClick={() => void confirmLogout()}
              >
                确认删除
              </button>
            </div>
          </section>
        </div>
      ) : null}
    </section>
  );
}
