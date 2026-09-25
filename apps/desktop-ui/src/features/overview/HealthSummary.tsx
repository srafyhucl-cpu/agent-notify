import { ArrowUpRight, Bot, Gauge, RadioTower } from "lucide-react";

import type {
  ChannelAccountDto,
  RuntimeSnapshotDto,
  RuntimeSummaryDto,
} from "../../bridge/types";
import { SafeLink } from "../../components/SafeLink";

export const RUNTIME_STATE_LABELS: Record<RuntimeSummaryDto["state"], string> = {
  Starting: "启动中",
  Running: "运行中",
  Paused: "已暂停",
  MigrationRequired: "迁移待处理",
  Stopping: "正在停止",
  Stopped: "已停止",
  Failed: "异常",
};

export function channelWaitingLogin(account: ChannelAccountDto): boolean {
  return (
    account.enabled &&
    !account.health.available &&
    !account.health.stale &&
    account.lastInboundAt === null
  );
}

export function channelInvalid(account: ChannelAccountDto): boolean {
  return !account.enabled || account.health.stale;
}

export function channelOnline(account: ChannelAccountDto): boolean {
  return account.enabled && account.health.available && !account.health.stale;
}

export interface HealthSummaryProps {
  snapshot: RuntimeSnapshotDto;
  pausePending: boolean;
  onTogglePause: () => void;
}

/**
 * 运行概况汇总卡：渠道账号 / Agent 接入 / 运行状态 三列共用一个容器。
 * 列顺序遵循产品原则「以渠道为主」——渠道健康居首，其数字为全页视觉锚点；
 * 前两列支持点击一键下钻到渠道与 Agent 模块，动线自然顺畅。
 */
export function HealthSummary({
  snapshot,
  pausePending,
  onTogglePause,
}: HealthSummaryProps) {
  const runtime = snapshot.runtime;
  const agents = snapshot.overview.agents;
  const channels = snapshot.overview.channels;
  const unhealthyAgents = agents.filter(
    (agent) => !agent.health.available,
  ).length;
  const onlineChannels = channels.filter(channelOnline).length;
  const waitingChannels = channels.filter(channelWaitingLogin).length;
  const invalidChannels = channels.filter(channelInvalid).length;

  return (
    <section className="overview-summary overview-summary-grid" aria-label="运行概况">
      <section
        className="overview-summary-section overview-metric-card"
        aria-labelledby="overview-channels"
      >
        <SafeLink
          to="/channels"
          className="overview-summary-section--link"
          aria-label="查看渠道账号详情"
        >
          <header className="overview-summary-header">
            <div className="overview-summary-title">
              <RadioTower aria-hidden="true" size={16} />
              <h2 id="overview-channels">渠道账号</h2>
            </div>
            <ArrowUpRight aria-hidden="true" size={16} />
          </header>
          <p className="overview-summary-value">
            {onlineChannels}
            <span className="overview-summary-suffix"> / {channels.length}</span>
          </p>
          <p className="overview-summary-detail">
            等待登录 {waitingChannels} · 失效 {invalidChannels}
          </p>
        </SafeLink>
      </section>

      <section
        className="overview-summary-section overview-metric-card"
        aria-labelledby="overview-agents"
      >
        <SafeLink
          to="/agents"
          className="overview-summary-section--link"
          aria-label="查看 Agent 接入详情"
        >
          <header className="overview-summary-header">
            <div className="overview-summary-title">
              <Bot aria-hidden="true" size={16} />
              <h2 id="overview-agents">Agent 接入</h2>
            </div>
            <ArrowUpRight aria-hidden="true" size={16} />
          </header>
          <p className="overview-summary-value">{agents.length}</p>
          <p className="overview-summary-detail">异常 {unhealthyAgents}</p>
        </SafeLink>
      </section>

      <section
        className="overview-summary-section overview-metric-card"
        aria-labelledby="overview-runtime"
      >
        <header className="overview-summary-header">
          <div className="overview-summary-title">
            <Gauge aria-hidden="true" size={16} />
            <h2 id="overview-runtime">运行状态</h2>
          </div>
          <button
            className="button button-secondary"
            type="button"
            disabled={pausePending}
            onClick={onTogglePause}
          >
            {pausePending ? "正在更新" : runtime.paused ? "恢复通知" : "暂停通知"}
          </button>
        </header>
        <p className="overview-summary-value">
          {RUNTIME_STATE_LABELS[runtime.state]}
        </p>
        <p className="overview-summary-detail">
          通知 {runtime.paused ? "已暂停" : "接收中"} · 平台 {runtime.platform}
        </p>
      </section>
    </section>
  );
}
