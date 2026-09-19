import { LogOut } from "lucide-react";

import type {
  ChannelAccountDto,
  ChannelCapabilitiesDto,
  ChannelDto,
} from "../../bridge/types";
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

export interface ChannelAccountDetailProps {
  channel: ChannelDto;
  account: ChannelAccountDto;
  pending: boolean;
  onToggle: (account: ChannelAccountDto, enabled: boolean) => void;
  onLogout: (account: ChannelAccountDto) => void;
}

export function ChannelAccountDetail({
  channel,
  account,
  pending,
  onToggle,
  onLogout,
}: ChannelAccountDetailProps) {
  return (
    <section
      className="channel-account-detail"
      aria-labelledby={`channel-account-detail-${account.id}`}
    >
      <header className="section-heading">
        <div>
          <h2 id={`channel-account-detail-${account.id}`}>
            {account.displayName} 详情
          </h2>
          <p className="section-description">
            {channel.displayName} · {channel.id}
          </p>
        </div>
        <button
          className="button button-secondary button-danger-text"
          type="button"
          aria-label={`退出账号 ${account.displayName}`}
          disabled={pending}
          onClick={() => onLogout(account)}
        >
          <LogOut aria-hidden="true" size={15} />
          退出账号 {account.displayName}
        </button>
      </header>

      <dl className="descriptor-list channel-descriptor-list">
        <div>
          <dt>账号 ID</dt>
          <dd className="monospace-cell">{account.id}</dd>
        </div>
        <div>
          <dt>启用状态</dt>
          <dd>{account.enabled ? "已启用" : "已停用"}</dd>
        </div>
        <div>
          <dt>最近入站</dt>
          <dd>{formatTime(account.lastInboundAt)}</dd>
        </div>
        <div>
          <dt>最近投递</dt>
          <dd>{formatTime(account.lastDeliveryAt)}</dd>
        </div>
        <div>
          <dt>当前健康</dt>
          <dd>{account.health.detail?.message ?? "无异常"}</dd>
        </div>
      </dl>

      <div className="channel-detail-actions">
        <button
          className="button button-secondary"
          type="button"
          disabled={pending}
          onClick={() => onToggle(account, !account.enabled)}
        >
          {account.enabled ? "停用此账号" : "启用此账号"}
        </button>
      </div>

      <div className="capability-list" aria-label="渠道能力">
        {(Object.entries(channel.capabilities) as Array<
          [
            keyof ChannelCapabilitiesDto,
            ChannelCapabilitiesDto[keyof ChannelCapabilitiesDto],
          ]
        >).map(([name, value]) => (
          <span className="capability-item capability-item--enabled" key={name}>
            {CAPABILITY_LABELS[name]}
            <strong>{capabilityValue(name, value)}</strong>
          </span>
        ))}
      </div>

      <div className="channel-config-section">
        <h3>账号配置</h3>
        <ChannelConfigForm channel={channel} account={account} />
      </div>
    </section>
  );
}
