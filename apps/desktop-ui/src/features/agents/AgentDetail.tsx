import { CircleCheck, CircleX } from "lucide-react";

import type { HostBridge } from "../../bridge";
import type {
  AgentCapabilitiesDto,
  AgentDto,
} from "../../bridge/types";
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
      <header className="section-heading">
        <div>
          <h2 id={`agent-detail-${agent.id}`}>{agent.displayName} 详情</h2>
          <p className="section-description">{agent.description}</p>
        </div>
        <span
          className={`status-label ${
            agent.health.available ? "status-label--success" : "status-label--danger"
          }`}
        >
          <HealthIcon aria-hidden="true" size={15} />
          {agent.health.available ? "服务可用" : "服务不可用"}
        </span>
      </header>

      <dl className="descriptor-list">
        <div>
          <dt>Agent ID</dt>
          <dd>{agent.id}</dd>
        </div>
        <div>
          <dt>启用状态</dt>
          <dd>{agent.enabled ? "已启用" : "已停用"}</dd>
        </div>
        <div>
          <dt>最近事件</dt>
          <dd>{agent.health.detail?.message ?? "无异常"}</dd>
        </div>
      </dl>

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

      <div className="agent-config-section">
        <h3>配置</h3>
        <AgentConfigForm key={agent.id} agent={agent} bridge={bridge} />
      </div>
    </section>
  );
}
