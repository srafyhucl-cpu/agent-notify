import type { DeliveryDto } from "../../bridge/types";

const MAX_RECENT_DELIVERIES = 20;
const DELIVERY_TIME_FORMATTER = new Intl.DateTimeFormat("zh-CN", {
  year: "numeric",
  month: "2-digit",
  day: "2-digit",
  hour: "2-digit",
  minute: "2-digit",
  second: "2-digit",
  hour12: false,
});

const DELIVERY_STATE_LABELS: Record<DeliveryDto["state"], string> = {
  Pending: "等待发送",
  Sent: "已发送",
  Failed: "发送失败",
  Unknown: "结果未知",
  Skipped: "已跳过",
};

function formatTime(value: string): string {
  const date = new Date(value);
  return Number.isNaN(date.getTime()) ? value : DELIVERY_TIME_FORMATTER.format(date);
}

export interface RecentDeliveriesProps {
  deliveries: DeliveryDto[];
}

export function RecentDeliveries({ deliveries }: RecentDeliveriesProps) {
  const recent = deliveries.slice(0, MAX_RECENT_DELIVERIES);

  return (
    <section className="overview-section" aria-labelledby="overview-deliveries">
      <header className="overview-section-header">
        <div className="overview-section-title">
          <h2 id="overview-deliveries">最近投递</h2>
          <span className="section-count">最近 {recent.length} 条</span>
        </div>
      </header>

      {recent.length === 0 ? (
        <p className="section-empty">最近没有投递记录。</p>
      ) : (
        <div className="table-scroll">
          <table className="data-table delivery-table">
            <thead>
              <tr>
                <th scope="col">Notification</th>
                <th scope="col">账号</th>
                <th scope="col">状态</th>
                <th scope="col">时间</th>
                <th scope="col">错误</th>
              </tr>
            </thead>
            <tbody>
              {recent.map((delivery) => (
                <tr key={delivery.id}>
                  <td className="monospace-cell">{delivery.notificationId}</td>
                  <td>{delivery.accountId}</td>
                  <td>
                    <span
                      className={`delivery-state delivery-state--${delivery.state.toLowerCase()}`}
                    >
                      {DELIVERY_STATE_LABELS[delivery.state]}
                    </span>
                  </td>
                  <td>{formatTime(delivery.updatedAt)}</td>
                  <td className="error-cell">{delivery.error?.message ?? "无"}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
    </section>
  );
}
