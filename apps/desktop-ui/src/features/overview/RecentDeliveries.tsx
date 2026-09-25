import type { DeliveryDto } from "../../bridge/types";
import { EmptyState } from "../../components/EmptyState";
import { SafeLink } from "../../components/SafeLink";
import {
  SectionCard,
  StatusBadge,
  type StatusBadgeTone,
} from "../../components/patterns";
import { useAccountNames } from "../../data/accountNames";

const MAX_RECENT_DELIVERIES = 20;
const MINUTE_MS = 60_000;
const HOUR_MS = 60 * MINUTE_MS;
const DAY_MS = 24 * HOUR_MS;
const MONTH_DAYS = 30;

const DELIVERY_STATE_LABELS: Record<DeliveryDto["state"], string> = {
  Pending: "等待发送",
  Sent: "已发送",
  Failed: "发送失败",
  Unknown: "结果未知",
  Skipped: "已跳过",
};

const DELIVERY_STATE_TONES: Record<DeliveryDto["state"], StatusBadgeTone> = {
  Pending: "warning",
  Sent: "success",
  Failed: "danger",
  Unknown: "danger",
  Skipped: "warning",
};

type DeliveryGroupKey = "today" | "yesterday" | "earlier";

interface DeliveryGroup {
  key: DeliveryGroupKey;
  label: string;
  deliveries: DeliveryDto[];
}

const DELIVERY_GROUPS: ReadonlyArray<{
  key: DeliveryGroupKey;
  label: string;
}> = [
  { key: "today", label: "今天" },
  { key: "yesterday", label: "昨天" },
  { key: "earlier", label: "更早" },
];

const ABSOLUTE_TIME_FORMATTER = new Intl.DateTimeFormat("zh-CN", {
  year: "numeric",
  month: "2-digit",
  day: "2-digit",
  hour: "2-digit",
  minute: "2-digit",
  hour12: false,
});

function startOfDay(time: number): number {
  const date = new Date(time);
  return new Date(date.getFullYear(), date.getMonth(), date.getDate()).getTime();
}

/** 按「今天 / 昨天 / 更早」分组；无效时间的投递归入「更早」。 */
function groupDeliveries(deliveries: DeliveryDto[], now: number): DeliveryGroup[] {
  const todayStart = startOfDay(now);
  const yesterdayStart = todayStart - DAY_MS;
  const groups: DeliveryGroup[] = DELIVERY_GROUPS.map((group) => ({
    ...group,
    deliveries: [],
  }));

  for (const delivery of deliveries) {
    const time = new Date(delivery.updatedAt).getTime();
    const index =
      Number.isNaN(time) || time < yesterdayStart
        ? 2
        : time < todayStart
          ? 1
          : 0;
    groups[index].deliveries.push(delivery);
  }

  return groups.filter((group) => group.deliveries.length > 0);
}

/** 相对时间：刚刚 / N 分钟前 / N 小时前 / N 天前；超过一个月退回绝对时间。 */
function formatRelativeTime(value: string, now: number): string {
  const time = new Date(value).getTime();
  if (Number.isNaN(time)) {
    return value;
  }
  const diff = now - time;
  if (diff < MINUTE_MS) {
    return "刚刚";
  }
  if (diff < HOUR_MS) {
    return `${String(Math.floor(diff / MINUTE_MS))} 分钟前`;
  }
  if (diff < DAY_MS) {
    return `${String(Math.floor(diff / HOUR_MS))} 小时前`;
  }
  const days = Math.floor(diff / DAY_MS);
  if (days < MONTH_DAYS) {
    return `${String(days)} 天前`;
  }
  return ABSOLUTE_TIME_FORMATTER.format(time);
}

export interface RecentDeliveriesProps {
  deliveries: DeliveryDto[];
}

export function RecentDeliveries({ deliveries }: RecentDeliveriesProps) {
  const { formatAccount } = useAccountNames();
  const recent = deliveries.slice(0, MAX_RECENT_DELIVERIES);
  const now = Date.now();
  const groups = groupDeliveries(recent, now);

  return (
    <SectionCard
      title="最近投递"
      count={recent.length}
      action={
        recent.length > 0 ? (
          <SafeLink className="button button-secondary no-underline" to="/history">
            查看全部
          </SafeLink>
        ) : undefined
      }
    >
      {recent.length === 0 ? (
        <EmptyState
          title="最近没有投递记录"
          description="连接渠道并触发通知后，这里才会出现投递记录。"
          action={
            <SafeLink className="button" to="/channels">
              去连接渠道
            </SafeLink>
          }
        />
      ) : (
        <div className="delivery-stream">
          {groups.map((group) => (
            <section
              className="delivery-group"
              key={group.key}
              aria-label={group.label}
            >
              <h3 className="delivery-group-label">{group.label}</h3>
              <ul className="delivery-rows">
                {group.deliveries.map((delivery) => (
                  <li key={delivery.id}>
                    <SafeLink
                      className="delivery-row delivery-row--interactive"
                      to="/history"
                      aria-label={`查看投递 ${delivery.notificationId} 历史详情`}
                    >
                      <div className="delivery-row-main">
                        <span className="delivery-row-account">
                          {formatAccount(delivery.accountId)}
                        </span>
                        <div className="delivery-row-sub">
                          <span className="delivery-row-title monospace-cell">
                            {delivery.notificationId}
                          </span>
                          {delivery.error?.message ? (
                            <span className="delivery-row-error">
                              · {delivery.error.message}
                            </span>
                          ) : null}
                        </div>
                      </div>
                      <span className="delivery-row-time">
                        {formatRelativeTime(delivery.updatedAt, now)}
                      </span>
                      <StatusBadge tone={DELIVERY_STATE_TONES[delivery.state]}>
                        {DELIVERY_STATE_LABELS[delivery.state]}
                      </StatusBadge>
                    </SafeLink>
                  </li>
                ))}
              </ul>
            </section>
          ))}
        </div>
      )}
    </SectionCard>
  );
}
