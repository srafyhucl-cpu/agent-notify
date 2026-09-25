import { History as HistoryIcon, Search, X } from "lucide-react";
import { useMemo, useState } from "react";
import { Link } from "react-router-dom";

import type { HostBridge } from "../../bridge";
import type { DeliveryDto, DeliveryStateDto } from "../../bridge/types";
import { EmptyState } from "../../components/EmptyState";
import { InlineError } from "../../components/InlineError";
import { LoadingRows } from "../../components/LoadingRows";
import { PageHeader } from "../../components/patterns";
import { toUserError } from "../../data/errors";
import { useAccountNames } from "../../data/accountNames";
import { useRetryDeliveryMutation } from "../../data/mutations";
import { useAgents } from "../../data/useAgents";
import { useChannels } from "../../data/useChannels";
import {
  type HistoryFilters,
  useHistory,
  useNotificationDetail,
} from "../../data/useHistory";
import { HistoryDetail } from "./HistoryDetail";
import { HistoryTable } from "./HistoryTable";

const STATUS_OPTIONS: Array<{ value: DeliveryStateDto; label: string }> = [
  { value: "Pending", label: "等待投递" },
  { value: "Sent", label: "已送达" },
  { value: "Failed", label: "投递失败" },
  { value: "Unknown", label: "未确认" },
  { value: "Skipped", label: "已跳过" },
];

function valueOrNull(value: string): string | null {
  return value.trim() === "" ? null : value;
}

function dateTimeToIso(value: string): string | null {
  if (!value) {
    return null;
  }
  const date = new Date(value);
  return Number.isNaN(date.getTime()) ? null : date.toISOString();
}

function hasActiveFilters(filters: HistoryFilters): boolean {
  return Object.values(filters).some((value) => value !== null);
}

export interface HistoryPageProps {
  bridge: HostBridge;
}

export function HistoryPage({ bridge }: HistoryPageProps) {
  const [agentId, setAgentId] = useState("");
  const [channelId, setChannelId] = useState("");
  const [accountId, setAccountId] = useState("");
  const [deliveryState, setDeliveryState] = useState("");
  const [from, setFrom] = useState("");
  const [to, setTo] = useState("");
  const [query, setQuery] = useState("");
  const [selectedNotificationId, setSelectedNotificationId] = useState<
    string | null
  >(null);
  const [retryError, setRetryError] = useState<unknown>(null);
  const [retryingDeliveryId, setRetryingDeliveryId] = useState<string | null>(
    null,
  );

  const filters = useMemo<HistoryFilters>(
    () => ({
      agentId: valueOrNull(agentId),
      channelId: valueOrNull(channelId),
      accountId: valueOrNull(accountId),
      deliveryState: valueOrNull(deliveryState) as DeliveryStateDto | null,
      from: dateTimeToIso(from),
      to: dateTimeToIso(to),
      query: valueOrNull(query),
    }),
    [accountId, agentId, channelId, deliveryState, from, query, to],
  );

  const historyQuery = useHistory(bridge, filters);
  const agentsQuery = useAgents(bridge);
  const channelsQuery = useChannels(bridge);
  const detailQuery = useNotificationDetail(bridge, selectedNotificationId);
  const retryMutation = useRetryDeliveryMutation(bridge);

  const notifications = useMemo(
    () => historyQuery.data?.pages.flatMap((page) => page.items) ?? [],
    [historyQuery.data],
  );
  const agentNames = useMemo(
    () =>
      new Map(
        (agentsQuery.data ?? []).map((agent) => [agent.id, agent.displayName]),
      ),
    [agentsQuery.data],
  );
  const { getDisplayName } = useAccountNames();
  const accounts = useMemo(
    () =>
      (channelsQuery.data?.channels ?? []).flatMap((channel) =>
        channel.accounts.map((account) => ({
          id: account.id,
          channelId: channel.id,
          label: `${channel.displayName} · ${getDisplayName(account)}`,
        })),
      ),
    [channelsQuery.data, getDisplayName],
  );
  const details = useMemo(
    () =>
      detailQuery.data
        ? { [detailQuery.data.notification.id]: detailQuery.data }
        : {},
    [detailQuery.data],
  );
  const loadError = historyQuery.error ? toUserError(historyQuery.error) : null;
  const total = historyQuery.data?.pages[0]?.total ?? notifications.length;

  const resetFilters = () => {
    setAgentId("");
    setChannelId("");
    setAccountId("");
    setDeliveryState("");
    setFrom("");
    setTo("");
    setQuery("");
  };

  const retryDelivery = async (delivery: DeliveryDto) => {
    setRetryError(null);
    setRetryingDeliveryId(delivery.id);
    try {
      await retryMutation.mutateAsync({ deliveryId: delivery.id });
    } catch (error) {
      setRetryError(error);
    } finally {
      setRetryingDeliveryId(null);
    }
  };

  return (
    <section className="workbench-page history-page" aria-label="历史">
      <PageHeader
        title="历史"
        summary="按 Agent、渠道、账号、状态和时间定位历史通知，再检查投递与路由结果。"
        actions={
          <span className="page-count" aria-label={`共 ${total} 条通知`}>
            <HistoryIcon aria-hidden="true" size={16} />
            {total} 条
          </span>
        }
      />

      <div className="workbench-page-content history-page-content">
        <form
          className="history-filters"
          aria-label="历史筛选"
          onSubmit={(event) => event.preventDefault()}
        >
          {/* 主筛选：一行四列，跨 Agent / 渠道 / 账号 / 状态 */}
          <div className="history-filter-primary">
            <label>
              <span>Agent</span>
              <select
                aria-label="Agent"
                value={agentId}
                onChange={(event) => setAgentId(event.currentTarget.value)}
              >
                <option value="">全部 Agent</option>
                {(agentsQuery.data ?? []).map((agent) => (
                  <option value={agent.id} key={agent.id}>
                    {agent.displayName}
                  </option>
                ))}
              </select>
            </label>

            <label>
              <span>渠道</span>
              <select
                aria-label="渠道"
                value={channelId}
                onChange={(event) => {
                  setChannelId(event.currentTarget.value);
                  setAccountId("");
                }}
              >
                <option value="">全部渠道</option>
                {(channelsQuery.data?.channels ?? []).map((channel) => (
                  <option value={channel.id} key={channel.id}>
                    {channel.displayName}
                  </option>
                ))}
              </select>
            </label>

            <label>
              <span>账号</span>
              <select
                aria-label="账号"
                value={accountId}
                onChange={(event) => setAccountId(event.currentTarget.value)}
              >
                <option value="">全部账号</option>
                {accounts
                  .filter(
                    (account) => !channelId || account.channelId === channelId,
                  )
                  .map((account) => (
                    <option value={account.id} key={account.id}>
                      {account.label}
                    </option>
                  ))}
              </select>
            </label>

            <label>
              <span>状态</span>
              <select
                aria-label="状态"
                value={deliveryState}
                onChange={(event) => setDeliveryState(event.currentTarget.value)}
              >
                <option value="">全部状态</option>
                {STATUS_OPTIONS.map((option) => (
                  <option value={option.value} key={option.value}>
                    {option.label}
                  </option>
                ))}
              </select>
            </label>
          </div>

          {/* 次筛选：折到第二行，重置并入行尾 */}
          <div className="history-filter-secondary">
            <label>
              <span>开始时间</span>
              <input
                aria-label="开始时间"
                type="datetime-local"
                value={from}
                onChange={(event) => setFrom(event.currentTarget.value)}
              />
            </label>

            <label>
              <span>结束时间</span>
              <input
                aria-label="结束时间"
                type="datetime-local"
                value={to}
                onChange={(event) => setTo(event.currentTarget.value)}
              />
            </label>

            <label className="history-keyword-filter">
              <span>关键词</span>
              <span className="history-search-control">
                <Search aria-hidden="true" size={15} />
                <input
                  aria-label="关键词"
                  type="search"
                  placeholder="正文或标题"
                  value={query}
                  onChange={(event) => setQuery(event.currentTarget.value)}
                />
              </span>
            </label>

            <button
              className="button button-secondary history-reset-button"
              type="button"
              disabled={!hasActiveFilters(filters)}
              onClick={resetFilters}
            >
              <X aria-hidden="true" size={15} />
              重置
            </button>
          </div>
        </form>

        {loadError ? (
          <InlineError
            title="无法读取历史通知"
            message={loadError.message}
            action={
              <button
                className="button button-secondary"
                type="button"
                onClick={() => void historyQuery.refetch()}
              >
                重新检查
              </button>
            }
          />
        ) : null}

        {historyQuery.isPending && !historyQuery.data ? (
          <LoadingRows aria-label="正在加载历史通知" rows={7} />
        ) : null}

        {!historyQuery.isPending && notifications.length === 0 && !loadError ? (
          <EmptyState
            title="暂无历史通知"
            description="当前筛选条件下没有通知记录。连接渠道并触发通知后，这里才会出现投递记录。"
            action={
              <Link className="button" to="/channels">
                去连接渠道
              </Link>
            }
          />
        ) : null}

        {notifications.length > 0 ? (
          <div className="history-workspace">
            <HistoryTable
              notifications={notifications}
              details={details}
              agentNames={agentNames}
              selectedId={selectedNotificationId}
              onSelect={setSelectedNotificationId}
              hasNextPage={historyQuery.hasNextPage}
              isFetchingNextPage={historyQuery.isFetchingNextPage}
              onLoadMore={() => void historyQuery.fetchNextPage()}
            />
            <HistoryDetail
              key={detailQuery.data?.notification.id ?? "empty"}
              detail={detailQuery.data ?? null}
              isLoading={detailQuery.isPending}
              loadError={detailQuery.error}
              retryError={retryError}
              retryingDeliveryId={retryingDeliveryId}
              onRetry={(delivery) => void retryDelivery(delivery)}
            />
          </div>
        ) : null}
      </div>
    </section>
  );
}
