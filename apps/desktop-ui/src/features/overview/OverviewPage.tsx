import { RefreshCw } from "lucide-react";
import { useState } from "react";

import type { HostBridge } from "../../bridge";
import type { RuntimeSnapshotDto } from "../../bridge/types";
import { EmptyState } from "../../components/EmptyState";
import { InlineError } from "../../components/InlineError";
import { LoadingRows } from "../../components/LoadingRows";
import {
  KpiCard,
  PageHeader,
  SectionCard,
  type KpiTone,
} from "../../components/patterns";
import { toUserError } from "../../data/errors";
import { useSetRuntimePausedMutation } from "../../data/mutations";
import { useSnapshot } from "../../data/useSnapshot";
import {
  HealthSummary,
  RUNTIME_STATE_LABELS,
  channelInvalid,
  channelOnline,
  channelWaitingLogin,
} from "./HealthSummary";
import { RecentDeliveries } from "./RecentDeliveries";

interface ActionItem {
  key: string;
  title: string;
  message: string;
}

function actionItems(snapshot: RuntimeSnapshotDto): ActionItem[] {
  const items: ActionItem[] = [];

  for (const agent of snapshot.overview.agents) {
    if (!agent.health.available) {
      items.push({
        key: `agent-${agent.id}`,
        title: `${agent.displayName} 接入异常`,
        message:
          agent.health.detail?.message ??
          "请检查 Agent 安装与版本后重新检查。",
      });
    }
  }

  for (const account of snapshot.overview.channels) {
    if (!account.enabled || !account.health.available || account.health.stale) {
      items.push({
        key: `channel-${account.id}`,
        title: `${account.displayName} 需要处理`,
        message:
          account.health.detail?.message ??
          (account.enabled
            ? "请到渠道页面检查登录状态或重新登录。"
            : "该账号已停用，如需接收或发送消息请重新启用。"),
      });
    }
  }

  for (const delivery of snapshot.overview.recentDeliveries) {
    if (
      delivery.state === "Failed" ||
      delivery.state === "Unknown" ||
      delivery.state === "Skipped"
    ) {
      items.push({
        key: `delivery-${delivery.id}`,
        title: `投递 ${delivery.notificationId} 未完成`,
        message:
          delivery.error?.message ??
          "请先检查原渠道是否已收到消息，再前往历史页面查看详情。",
      });
    }
  }

  if (snapshot.overview.storage.recentError) {
    items.push({
      key: "storage-latest-error",
      title: "最近一次数据操作失败",
      message: `${snapshot.overview.storage.recentError.message} 请先备份数据，再到诊断页面检查。`,
    });
  }

  for (const diagnostic of snapshot.diagnostics) {
    if (diagnostic.level !== "Normal") {
      items.push({
        key: `diagnostic-${diagnostic.code}`,
        title: diagnostic.code,
        message: diagnostic.action
          ? `${diagnostic.message} 下一步：${diagnostic.action.label}。`
          : diagnostic.message,
      });
    }
  }

  return items;
}

/** 运行状态 KPI 语义色：正常 / 等待 / 异常三档（颜色 + 文字双载）。 */
function runtimeKpiTone(state: RuntimeSnapshotDto["runtime"]["state"]): KpiTone {
  switch (state) {
    case "Running":
      return "success";
    case "Failed":
    case "Stopped":
      return "danger";
    case "Starting":
    case "Paused":
    case "MigrationRequired":
    case "Stopping":
      return "warning";
    default:
      return "default";
  }
}

export interface OverviewPageProps {
  bridge: HostBridge;
}

export function OverviewPage({ bridge }: OverviewPageProps) {
  const snapshotQuery = useSnapshot(bridge);
  const pauseMutation = useSetRuntimePausedMutation(bridge);
  const [pauseError, setPauseError] = useState<unknown>(null);
  const snapshot = snapshotQuery.data;
  const loadError = snapshotQuery.error
    ? toUserError(snapshotQuery.error)
    : null;
  const actionError = pauseError ? toUserError(pauseError) : null;
  const items = snapshot ? actionItems(snapshot) : [];
  // KPI 锚点带：只取现有快照字段直接可得的指标，不发明新度量。
  const agents = snapshot?.overview.agents ?? [];
  const channels = snapshot?.overview.channels ?? [];
  const onlineChannels = channels.filter(channelOnline).length;
  const waitingChannels = channels.filter(channelWaitingLogin).length;
  const invalidChannels = channels.filter(channelInvalid).length;
  const unhealthyAgents = agents.filter(
    (agent) => !agent.health.available,
  ).length;
  const channelTone: KpiTone =
    channels.length === 0
      ? "default"
      : invalidChannels > 0
        ? "danger"
        : waitingChannels > 0
          ? "warning"
          : "success";
  const agentTone: KpiTone =
    unhealthyAgents > 0 ? "danger" : agents.length > 0 ? "success" : "default";
  const runtimeTone: KpiTone = snapshot
    ? runtimeKpiTone(snapshot.runtime.state)
    : "default";
  const pendingTone: KpiTone = items.length > 0 ? "warning" : "success";

  const togglePause = async () => {
    if (!snapshot) {
      return;
    }
    setPauseError(null);
    try {
      await pauseMutation.mutateAsync({ paused: !snapshot.runtime.paused });
    } catch (error) {
      setPauseError(error);
    }
  };

  return (
    <section className="workbench-page overview-page" aria-label="总览">
      <PageHeader
        title="总览"
        summary="查看运行状态、接入健康、最近投递和待处理故障。"
        actions={
          <button
            className="button button-secondary"
            type="button"
            disabled={snapshotQuery.isFetching}
            onClick={() => void snapshotQuery.refetch()}
          >
            <RefreshCw aria-hidden="true" size={16} />
            {snapshotQuery.isFetching ? "正在检查" : "重新检查"}
          </button>
        }
      />

      <div className="workbench-page-content overview-page-content">
        {loadError ? (
          <InlineError
            title="无法读取总览"
            message={loadError.message}
            action={
              <button
                className="button button-secondary"
                type="button"
                onClick={() => void snapshotQuery.refetch()}
              >
                重新检查
              </button>
            }
          />
        ) : null}

        {actionError ? (
          <InlineError title={actionError.title} message={actionError.message} />
        ) : null}

        {snapshotQuery.isPending && !snapshot ? (
          <LoadingRows aria-label="正在加载总览" rows={4} />
        ) : null}

        {!snapshotQuery.isPending && !snapshot && !loadError ? (
          <EmptyState description="当前没有可显示的总览数据。" />
        ) : null}

        {snapshot ? (
          <>
            {/* KPI 锚点带：渠道账号健康居首，其后 Agent 接入 / 运行状态 / 待处理 */}
            <div className="overview-kpi-band">
              <KpiCard
                value={onlineChannels}
                suffix={`/ ${String(channels.length)}`}
                label={`渠道账号在线 · 等待登录 ${String(waitingChannels)} · 失效 ${String(invalidChannels)}`}
                tone={channelTone}
              />
              <KpiCard
                value={agents.length}
                suffix={
                  unhealthyAgents > 0
                    ? `· ${String(unhealthyAgents)} 异常`
                    : undefined
                }
                label="Agent 接入"
                tone={agentTone}
              />
              <KpiCard
                value={RUNTIME_STATE_LABELS[snapshot.runtime.state]}
                label="运行状态"
                tone={runtimeTone}
              />
              <KpiCard value={items.length} label="待处理" tone={pendingTone} />
            </div>

            <HealthSummary
              snapshot={snapshot}
              pausePending={pauseMutation.isPending}
              onTogglePause={() => void togglePause()}
            />

            <RecentDeliveries deliveries={snapshot.overview.recentDeliveries} />

            <SectionCard title="需要处理" count={items.length}>
              {items.length === 0 ? (
                <p className="section-empty">当前没有需要处理的故障。</p>
              ) : (
                <ul className="action-list">
                  {items.map((item) => (
                    <li key={item.key}>
                      <strong>{item.title}</strong>
                      <span>{item.message}</span>
                    </li>
                  ))}
                </ul>
              )}
            </SectionCard>
          </>
        ) : null}
      </div>
    </section>
  );
}
