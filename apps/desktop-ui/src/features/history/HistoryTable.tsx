import { useVirtualizer } from "@tanstack/react-virtual";
import { useRef } from "react";

import type {
  DeliveryStateDto,
  NotificationDetailDto,
  NotificationSummaryDto,
} from "../../bridge/types";
import {
  StatusBadge,
  type StatusBadgeTone,
} from "../../components/patterns";

const HISTORY_ROW_HEIGHT = 54;
const HISTORY_OVERSCAN = 8;
const TEST_VIEWPORT_RECT = { width: 1080, height: 540 };
const INITIAL_FALLBACK_ROW_COUNT = 20;

const DELIVERY_STATE_LABELS: Record<DeliveryStateDto, string> = {
  Pending: "等待投递",
  Sent: "已送达",
  Failed: "投递失败",
  Unknown: "投递结果未确认",
  Skipped: "已跳过",
};

function formatTimestamp(value: string): string {
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) {
    return value;
  }
  return date.toLocaleString("zh-CN", {
    year: "numeric",
    month: "2-digit",
    day: "2-digit",
    hour: "2-digit",
    minute: "2-digit",
  });
}

export function historyStateLabel(states: DeliveryStateDto[]): string {
  if (states.length === 0) {
    return "未生成投递";
  }
  return states.map((state) => DELIVERY_STATE_LABELS[state]).join("、");
}

export function historyErrorSummary(states: DeliveryStateDto[]): string {
  if (states.includes("Unknown")) {
    return "请先检查原渠道是否已收到消息";
  }
  if (states.includes("Failed")) {
    return "投递失败，请在详情中确认是否重试";
  }
  if (states.includes("Pending")) {
    return "等待投递结果";
  }
  return "无可见错误";
}

function deliveryStateTone(states: DeliveryStateDto[]): StatusBadgeTone {
  if (states.includes("Unknown") || states.includes("Failed")) {
    return "danger";
  }
  if (states.includes("Pending") || states.includes("Skipped")) {
    return "warning";
  }
  if (states.length === 0) {
    return "neutral";
  }
  return "success";
}

export interface HistoryTableProps {
  notifications: NotificationSummaryDto[];
  details: Record<string, NotificationDetailDto>;
  agentNames: Map<string, string>;
  selectedId: string | null;
  onSelect: (notificationId: string) => void;
  hasNextPage: boolean;
  isFetchingNextPage: boolean;
  onLoadMore: () => void;
}

export function HistoryTable({
  notifications,
  details,
  agentNames,
  selectedId,
  onSelect,
  hasNextPage,
  isFetchingNextPage,
  onLoadMore,
}: HistoryTableProps) {
  const scrollRef = useRef<HTMLDivElement>(null);
  const virtualizer = useVirtualizer({
    count: notifications.length,
    getScrollElement: () => scrollRef.current,
    estimateSize: () => HISTORY_ROW_HEIGHT,
    overscan: HISTORY_OVERSCAN,
    initialRect: TEST_VIEWPORT_RECT,
    getItemKey: (index) => notifications[index]?.id ?? index,
  });
  const measuredRows = virtualizer.getVirtualItems();
  const visibleRows =
    measuredRows.length > 0
      ? measuredRows
      : Array.from(
          { length: Math.min(notifications.length, INITIAL_FALLBACK_ROW_COUNT) },
          (_, index) => ({
            index,
            start: index * HISTORY_ROW_HEIGHT,
            size: HISTORY_ROW_HEIGHT,
          }),
        );

  return (
    <div className="history-table-wrap">
      <div
        className="history-table"
        role="table"
        aria-label="历史通知列表"
        aria-rowcount={notifications.length}
      >
        <div className="history-row history-row--header" role="row">
          <span role="columnheader">时间</span>
          <span role="columnheader">Agent</span>
          <span role="columnheader">会话</span>
          <span role="columnheader">渠道账号</span>
          <span role="columnheader">状态</span>
          <span role="columnheader">错误摘要</span>
        </div>

        <div
          className="history-virtual-viewport"
          ref={scrollRef}
          role="rowgroup"
          aria-label="可滚动历史记录"
        >
          <div
            className="history-virtual-space"
            style={{ height: `${virtualizer.getTotalSize()}px` }}
          >
            {visibleRows.map((virtualRow) => {
              const notification = notifications[virtualRow.index];
              if (!notification) {
                return null;
              }
              const detail = details[notification.id];
              const delivery = detail?.deliveries[0] ?? null;
              const channelAccount = delivery
                ? `${delivery.channelId} / ${delivery.accountId}`
                : "查看详情";
              const selected = notification.id === selectedId;
              // 错误摘要与状态徽标同源：仅真错误（Unknown/Failed）用 danger，
              // 占位与等待类文案用弱化色，避免语义色误用。
              const stateTone = deliveryStateTone(notification.deliveryStates);

              return (
                <div
                  className={`history-row${selected ? " history-row--selected" : ""}`}
                  role="row"
                  aria-rowindex={virtualRow.index + 2}
                  data-index={virtualRow.index}
                  key={notification.id}
                  style={{
                    height: `${virtualRow.size}px`,
                    transform: `translateY(${virtualRow.start}px)`,
                  }}
                >
                  <span className="history-time-cell" role="cell">
                    {formatTimestamp(notification.occurredAt)}
                  </span>
                  <span role="cell" title={notification.agentId}>
                    {agentNames.get(notification.agentId) ?? notification.agentId}
                  </span>
                  <span className="history-session-cell" role="cell">
                    <button
                      className="history-title-button"
                      type="button"
                      aria-pressed={selected}
                      aria-label={notification.title}
                      onClick={() => onSelect(notification.id)}
                    >
                      <span className="history-title-text">
                        {notification.title}
                      </span>
                      <span className="history-session-text">
                        {notification.sessionTitle ??
                          notification.sessionId ??
                          "无会话"}
                      </span>
                    </button>
                  </span>
                  <span role="cell" title={channelAccount}>
                    {channelAccount}
                  </span>
                  <span className="history-state-cell" role="cell">
                    <StatusBadge tone={stateTone}>
                      {historyStateLabel(notification.deliveryStates)}
                    </StatusBadge>
                  </span>
                  <span
                    className={`history-error-cell${
                      stateTone === "danger" ? "" : " history-error-cell--muted"
                    }`}
                    role="cell"
                  >
                    {historyErrorSummary(notification.deliveryStates)}
                  </span>
                </div>
              );
            })}
          </div>
        </div>
      </div>

      {hasNextPage ? (
        <button
          className="button button-secondary history-load-more"
          type="button"
          disabled={isFetchingNextPage}
          onClick={onLoadMore}
        >
          {isFetchingNextPage ? "正在加载" : "加载更多"}
        </button>
      ) : null}
    </div>
  );
}
