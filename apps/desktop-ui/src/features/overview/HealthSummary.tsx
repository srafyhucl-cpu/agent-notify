import { Bot, Gauge, RadioTower } from "lucide-react";

import type {
  ChannelAccountDto,
  RuntimeSnapshotDto,
  RuntimeSummaryDto,
} from "../../bridge/types";

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
    <div className="overview-health">
      <section className="overview-health-section" aria-labelledby="overview-runtime">
        <header className="overview-health-header">
          <div className="overview-health-title">
            <Gauge aria-hidden="true" size={17} />
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
        <dl className="metric-list">
          <div>
            <dt>运行状态</dt>
            <dd>{RUNTIME_STATE_LABELS[runtime.state]}</dd>
          </div>
          <div>
            <dt>通知</dt>
            <dd>{runtime.paused ? "已暂停" : "接收中"}</dd>
          </div>
          <div>
            <dt>版本</dt>
            <dd>{runtime.appVersion}</dd>
          </div>
          <div>
            <dt>平台</dt>
            <dd>{runtime.platform}</dd>
          </div>
        </dl>
      </section>

      <section className="overview-health-section" aria-labelledby="overview-agents">
        <header className="overview-health-header">
          <div className="overview-health-title">
            <Bot aria-hidden="true" size={17} />
            <h2 id="overview-agents">Agent 接入</h2>
          </div>
        </header>
        <dl className="metric-list">
          <div>
            <dt>已接入</dt>
            <dd>{agents.length}</dd>
          </div>
          <div>
            <dt>异常</dt>
            <dd>{unhealthyAgents}</dd>
          </div>
        </dl>
      </section>

      <section className="overview-health-section" aria-labelledby="overview-channels">
        <header className="overview-health-header">
          <div className="overview-health-title">
            <RadioTower aria-hidden="true" size={17} />
            <h2 id="overview-channels">渠道账号</h2>
          </div>
        </header>
        <dl className="metric-list">
          <div>
            <dt>在线</dt>
            <dd>{onlineChannels}</dd>
          </div>
          <div>
            <dt>等待登录</dt>
            <dd>{waitingChannels}</dd>
          </div>
          <div>
            <dt>失效</dt>
            <dd>{invalidChannels}</dd>
          </div>
        </dl>
      </section>
    </div>
  );
}
