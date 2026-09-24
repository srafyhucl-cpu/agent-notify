import { CircleCheck, CircleX } from "lucide-react";

import type { HostBridge } from "../../bridge";
import type {
  AgentCapabilitiesDto,
  AgentDto,
} from "../../bridge/types";
import {
  FieldRow,
  SectionCard,
  StatusBadge,
} from "../../components/patterns";
import { AgentConfigForm } from "./AgentConfigForm";

const CAPABILITY_LABELS: Record<keyof AgentCapabilitiesDto, string> = {
  notify: "通知",
  resume: "续聊",
  sessionTitle: "会话标题",
  hookInstaller: "Hook 安装",
  replyWindow: "回复窗口",
};

export interface AgentDetailProps {
  agent: AgentDto;
  bridge: HostBridge;
}

export function AgentDetail({ agent, bridge }: AgentDetailProps) {
  const HealthIcon = agent.health.available ? CircleCheck : CircleX;

  return (
    <section className="agent-detail" aria-labelledby={`agent-detail-${agent.id}`}>
      <header className="agent-detail-header">
        <div className="agent-detail-heading">
          <h2 className="agent-detail-title" id={`agent-detail-${agent.id}`}>
            {agent.displayName} 详情
          </h2>
          <p className="agent-detail-description">{agent.description}</p>
        </div>
      </header>

      <div className="agent-detail-segments">
        <SectionCard
          title="身份"
          action={
            <StatusBadge
              className="agent-detail-health"
              tone={agent.health.available ? "success" : "danger"}
            >
              <HealthIcon aria-hidden="true" size={15} />
              {agent.health.available ? "服务可用" : "服务不可用"}
            </StatusBadge>
          }
        >
          <FieldRow
            label="Agent ID"
            control={<span className="monospace-cell">{agent.id}</span>}
          />
          <FieldRow
            label="启用状态"
            control={<span>{agent.enabled ? "已启用" : "已停用"}</span>}
          />
          <FieldRow
            label="最近事件"
            control={<span>{agent.health.detail?.message ?? "无异常"}</span>}
          />
        </SectionCard>

        <SectionCard title="能力">
          <div className="capability-list" aria-label="Agent 能力">
            {Object.entries(agent.capabilities).map(([name, enabled]) => (
              <span
                className={`capability-item ${enabled ? "capability-item--enabled" : ""}`}
                key={name}
              >
                {CAPABILITY_LABELS[name as keyof AgentCapabilitiesDto]}
                <strong>{enabled ? "可用" : "关闭"}</strong>
              </span>
            ))}
          </div>
        </SectionCard>

        <SectionCard title="配置">
          <AgentConfigForm key={agent.id} agent={agent} bridge={bridge} />
        </SectionCard>
      </div>
    </section>
  );
}
