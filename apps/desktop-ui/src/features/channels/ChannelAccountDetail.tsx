import type {
  ChannelAccountDto,
  ChannelCapabilitiesDto,
  ChannelDto,
} from "../../bridge/types";
import {
  StatusBadge,
  type StatusBadgeTone,
} from "../../components/patterns";
import { getAccountDisplayName } from "../../data/accountNames";
import { ChannelConfigForm } from "./ChannelConfigForm";

const CAPABILITY_LABELS: Record<keyof ChannelCapabilitiesDto, string> = {
  sendText: "发送文本",
  receive: "接收消息",
  replyRouting: "回复路由",
  editMessage: "编辑消息",
  attachments: "附件",
  markdown: "Markdown",
  maxTextBytes: "文本上限",
  inboundModes: "入站模式",
};

function formatTime(value: string | null): string {
  if (!value) {
    return "无记录";
  }

  const date = new Date(value);
  return Number.isNaN(date.getTime())
    ? value
    : date.toLocaleString("zh-CN", { hour12: false });
}

function capabilityValue(
  name: keyof ChannelCapabilitiesDto,
  value: ChannelCapabilitiesDto[keyof ChannelCapabilitiesDto],
): string {
  if (name === "maxTextBytes") {
    return value === null ? "未限制" : `${String(value)} 字节`;
  }
  if (name === "inboundModes") {
    return Array.isArray(value) && value.length > 0
      ? value.join("、")
      : "未声明";
  }
  return value ? "可用" : "关闭";
}

function detailHealth(account: ChannelAccountDto): {
  label: string;
  tone: StatusBadgeTone;
} {
  if (!account.enabled) {
    return { label: "已停用", tone: "warning" };
  }
  if (account.health.stale) {
    return { label: "需重新登录", tone: "danger" };
  }
  if (!account.health.available) {
    return { label: "登录异常", tone: "danger" };
  }
  return { label: "服务正常", tone: "success" };
}

export interface ChannelAccountDetailProps {
  channel: ChannelDto;
  account: ChannelAccountDto;
  pending?: boolean;
  onLogout?: (account: ChannelAccountDto) => void;
  onToggle?: (account: ChannelAccountDto, enabled: boolean) => void;
}

export function ChannelAccountDetail({
  channel,
  account,
}: ChannelAccountDetailProps) {
  const health = detailHealth(account);
  const displayName = getAccountDisplayName(account);

  return (
    <div
      className="channel-account-flat-detail"
      aria-label={`${displayName} 详情`}
    >
      <div className="channel-detail-flat-header">
        <div className="channel-detail-flat-title">
          <h3 className="channel-detail-flat-name">
            {displayName} 详情
          </h3>
          <span className="channel-detail-flat-sub">
            {channel.displayName} · {channel.id}
          </span>
        </div>
      </div>

      <div className="channel-detail-flat-grid">
        <div className="channel-detail-flat-pane">
          <h4 className="channel-detail-pane-title">连接指标</h4>
          <div className="channel-metrics-grid">
            <div className="channel-metric-cell">
              <span className="channel-metric-label">账号 ID</span>
              <span className="channel-metric-value monospace-cell">{account.id}</span>
            </div>
            <div className="channel-metric-cell">
              <span className="channel-metric-label">通道协议</span>
              <span className="channel-metric-value">{channel.displayName}</span>
            </div>
            <div className="channel-metric-cell">
              <span className="channel-metric-label">最近接收</span>
              <span className="channel-metric-value">{formatTime(account.lastInboundAt)}</span>
            </div>
            <div className="channel-metric-cell">
              <span className="channel-metric-label">最近投递</span>
              <span className="channel-metric-value">{formatTime(account.lastDeliveryAt)}</span>
            </div>
          </div>

          <div className="channel-detail-health-banner">
            <span className={`channel-health-dot channel-health-dot--${health.tone}`} aria-hidden="true" />
            <span className="channel-detail-health-desc">
              {account.health.detail?.message ?? "链路就绪，实时通信正常"}
            </span>
          </div>
        </div>

        <div className="channel-detail-flat-pane">
          <h4 className="channel-detail-pane-title">账号配置</h4>
          <div className="channel-detail-pane-body">
            <ChannelConfigForm channel={channel} account={account} />
          </div>
        </div>

        <div className="channel-detail-flat-pane channel-detail-flat-pane--full">
          <h4 className="channel-detail-pane-title">渠道特性与能力</h4>
          <div className="capability-list" aria-label="渠道能力">
            {(Object.entries(channel.capabilities) as Array<
              [
                keyof ChannelCapabilitiesDto,
                ChannelCapabilitiesDto[keyof ChannelCapabilitiesDto],
              ]
            >).map(([name, value]) => (
              <span
                className="capability-item capability-item--enabled"
                key={name}
              >
                <span className="capability-name">{CAPABILITY_LABELS[name]}</span>
                <span className="capability-val">{capabilityValue(name, value)}</span>
              </span>
            ))}
          </div>
        </div>
      </div>
    </div>
  );
}
