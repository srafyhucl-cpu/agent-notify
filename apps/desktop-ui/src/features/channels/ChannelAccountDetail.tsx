import { LogOut } from "lucide-react";

import type {
  ChannelAccountDto,
  ChannelCapabilitiesDto,
  ChannelDto,
} from "../../bridge/types";
import {
  FieldRow,
  SectionCard,
  StatusBadge,
  type StatusBadgeTone,
} from "../../components/patterns";
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

/** 详情健康徽标用词与列表行区分，避免同一文案在同一屏出现两次。 */
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
  return { label: "正常", tone: "success" };
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
  const health = detailHealth(account);

  return (
    <SectionCard
      className="channel-account-detail"
      title={`${account.displayName} 详情`}
      description={`${channel.displayName} · ${channel.id}`}
      action={
        <>
          <button
            className="button button-secondary"
            type="button"
            disabled={pending}
            onClick={() => onToggle(account, !account.enabled)}
          >
            {account.enabled ? "停用此账号" : "启用此账号"}
          </button>
          <button
            className="button button-secondary button-danger-text"
            type="button"
            aria-label={`退出账号 ${account.displayName}`}
            disabled={pending}
            onClick={() => onLogout(account)}
          >
            <LogOut aria-hidden="true" size={15} />
            退出账号
          </button>
        </>
      }
    >
      <div className="channel-detail-segments">
        <section className="channel-detail-segment">
          <h3 className="channel-detail-segment-title">身份</h3>
          <FieldRow
            label="账号 ID"
            control={<span className="monospace-cell">{account.id}</span>}
          />
          <FieldRow
            label="启用状态"
            control={<span>{account.enabled ? "已启用" : "已停用"}</span>}
          />
          <FieldRow
            label="最近入站"
            control={<span>{formatTime(account.lastInboundAt)}</span>}
          />
          <FieldRow
            label="最近投递"
            control={<span>{formatTime(account.lastDeliveryAt)}</span>}
          />
        </section>

        <section className="channel-detail-segment">
          <h3 className="channel-detail-segment-title">健康与错误</h3>
          <div className="channel-detail-health">
            <StatusBadge tone={health.tone}>{health.label}</StatusBadge>
            <span className="channel-detail-health-message">
              {account.health.detail?.message ?? "无异常"}
            </span>
          </div>
        </section>

        <section className="channel-detail-segment">
          <h3 className="channel-detail-segment-title">账号配置</h3>
          <ChannelConfigForm channel={channel} account={account} />
        </section>

        <section className="channel-detail-segment">
          <h3 className="channel-detail-segment-title">渠道能力</h3>
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
                {CAPABILITY_LABELS[name]}
                <strong>{capabilityValue(name, value)}</strong>
              </span>
            ))}
          </div>
        </section>
      </div>
    </SectionCard>
  );
}
