import { ChevronDown, ChevronUp, RotateCcw } from "lucide-react";
import { useState } from "react";

import type { DeliveryDto, NotificationDetailDto } from "../../bridge/types";
import { InlineError } from "../../components/InlineError";
import { LoadingRows } from "../../components/LoadingRows";
import {
  SectionCard,
  StatusBadge,
  type StatusBadgeTone,
} from "../../components/patterns";
import { toUserError } from "../../data/errors";
import { historyStateLabel } from "./HistoryTable";

function deliveryError(delivery: DeliveryDto) {
  return toUserError(
    delivery.error
      ? { ...delivery.error, retryable: false }
      : {
          code: "delivery_error_missing",
          message: "投递失败，但没有可显示的安全错误信息。",
          retryable: false,
        },
    { delivery: { id: delivery.id, state: delivery.state } },
  );
}

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
    second: "2-digit",
  });
}

function ExistenceValue({ exists }: { exists: boolean }) {
  return (
    <strong className={exists ? "existence-value--yes" : "existence-value--no"}>
      {exists ? "存在" : "不存在"}
    </strong>
  );
}

function deliveryStateTone(state: DeliveryDto["state"]): StatusBadgeTone {
  if (state === "Failed" || state === "Unknown") {
    return "danger";
  }
  if (state === "Pending" || state === "Skipped") {
    return "warning";
  }
  return "success";
}

export interface HistoryDetailProps {
  detail: NotificationDetailDto | null;
  isLoading: boolean;
  loadError: unknown;
  retryError: unknown;
  retryingDeliveryId: string | null;
  onRetry: (delivery: DeliveryDto) => void;
}

export function HistoryDetail({
  detail,
  isLoading,
  loadError,
  retryError,
  retryingDeliveryId,
  onRetry,
}: HistoryDetailProps) {
  const [bodyExpanded, setBodyExpanded] = useState(false);

  if (isLoading && !detail) {
    return (
      <section className="history-detail" aria-label="通知详情">
        <LoadingRows aria-label="正在加载通知详情" rows={4} />
      </section>
    );
  }

  if (loadError) {
    const userError = toUserError(loadError);
    return (
      <section className="history-detail" aria-label="通知详情">
        <InlineError title={userError.title} message={userError.message} />
      </section>
    );
  }

  if (!detail) {
    return (
      <section className="history-detail history-detail--empty" aria-label="通知详情">
        <h2>通知详情</h2>
        <p>选择一条历史记录查看投递与路由状态。</p>
      </section>
    );
  }

  const actionError = retryError ? toUserError(retryError) : null;

  return (
    <section className="history-detail" aria-label="通知详情">
      <header className="section-heading history-detail-heading">
        <div>
          <h2>{detail.notification.title}</h2>
          <p className="section-description">
            {formatTimestamp(detail.notification.occurredAt)}
          </p>
        </div>
        <StatusBadge tone="neutral">
          {historyStateLabel(detail.notification.deliveryStates)}
        </StatusBadge>
      </header>

      <SectionCard title="存在性" className="history-existence-card">
        <dl className="history-existence-list">
          <div>
            <dt>通知</dt>
            <dd>
              <ExistenceValue exists />
            </dd>
          </div>
          <div>
            <dt>投递</dt>
            <dd>
              {detail.deliveries.length > 0 ? (
                <>存在（{detail.deliveries.length} 条）</>
              ) : (
                <ExistenceValue exists={false} />
              )}
            </dd>
          </div>
          <div>
            <dt>路由</dt>
            <dd>
              <ExistenceValue exists={detail.routeExists} />
            </dd>
          </div>
        </dl>
      </SectionCard>

      <SectionCard title="正文" className="history-body-card">
        <div className="history-body-section">
          <button
            className="history-body-toggle"
            type="button"
            aria-expanded={bodyExpanded}
            onClick={() => setBodyExpanded((current) => !current)}
          >
            {bodyExpanded ? (
              <ChevronUp aria-hidden="true" size={16} />
            ) : (
              <ChevronDown aria-hidden="true" size={16} />
            )}
            {bodyExpanded ? "收起正文" : "展开正文"}
          </button>
          {bodyExpanded ? <pre className="history-body">{detail.body}</pre> : null}
        </div>
      </SectionCard>

      {actionError ? (
        <InlineError title={actionError.title} message={actionError.message} />
      ) : null}

      <SectionCard
        title="投递记录"
        count={detail.deliveries.length}
        className="history-deliveries"
      >
        {detail.deliveries.length === 0 ? (
          <p className="section-empty">没有投递记录。</p>
        ) : (
          detail.deliveries.map((delivery) => {
            const safeError = deliveryError(delivery);
            // 只有失败且明确标记为可重试的投递才允许人工重发。
            const canRetry = delivery.state === "Failed" && delivery.retryable;
            return (
              <article className="history-delivery" key={delivery.id}>
                <div className="history-delivery-header">
                  <div>
                    <strong>{delivery.channelId}</strong>
                    <span>{delivery.accountId}</span>
                  </div>
                  <StatusBadge tone={deliveryStateTone(delivery.state)}>
                    {historyStateLabel([delivery.state])}
                  </StatusBadge>
                </div>
                <p>
                  更新时间：{formatTimestamp(delivery.updatedAt)}
                  {delivery.externalMessageId
                    ? `；外部消息 ID：${delivery.externalMessageId}`
                    : ""}
                </p>
                {delivery.state === "Unknown" || delivery.error ? (
                  <div className="history-delivery-error">
                    <strong>{safeError.title}</strong>
                    <span>{safeError.message}</span>
                  </div>
                ) : null}
                {canRetry ? (
                  <div className="history-retry">
                    <p>仅在明确需要时手动重试，不会自动重发。</p>
                    <button
                      className="button button-secondary"
                      type="button"
                      disabled={retryingDeliveryId === delivery.id}
                      onClick={() => onRetry(delivery)}
                    >
                      <RotateCcw aria-hidden="true" size={15} />
                      {retryingDeliveryId === delivery.id ? "正在重试" : "重试"}
                    </button>
                  </div>
                ) : null}
              </article>
            );
          })
        )}
      </SectionCard>
    </section>
  );
}
