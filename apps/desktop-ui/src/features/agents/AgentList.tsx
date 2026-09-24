import { Settings2 } from "lucide-react";

import type { AgentDto } from "../../bridge/types";
import { StatusBadge, type StatusBadgeTone } from "../../components/patterns";

export interface AgentListProps {
  agents: AgentDto[];
  selectedAgentId: string | null;
  pendingAgentId: string | null;
  onSelect: (agentId: string) => void;
  onToggle: (agent: AgentDto, enabled: boolean) => void;
}

/** 接入状态徽标：文案保持列表既有用词，语义由「颜色 + 文字」双载。 */
function agentStatus(agent: AgentDto): { label: string; tone: StatusBadgeTone } {
  return agent.health.available
    ? { label: "接入正常", tone: "success" }
    : { label: "接入异常", tone: "danger" };
}

export function AgentList({
  agents,
  selectedAgentId,
  pendingAgentId,
  onSelect,
  onToggle,
}: AgentListProps) {
  return (
    <div className="agent-list" aria-label="Agent 列表">
      <table>
        <caption className="visually-hidden">Agent 接入与能力列表</caption>
        <thead>
          <tr>
            <th scope="col">名称</th>
            <th scope="col">通知</th>
            <th scope="col">接入状态</th>
            <th scope="col">配置</th>
          </tr>
        </thead>
        <tbody>
          {agents.map((agent) => {
            const status = agentStatus(agent);
            return (
              <tr
                className={
                  agent.id === selectedAgentId
                    ? "agent-row--selected"
                    : undefined
                }
                key={agent.id}
              >
                <th scope="row">
                  <button
                    className="agent-name-button"
                    type="button"
                    aria-pressed={agent.id === selectedAgentId}
                    onClick={() => onSelect(agent.id)}
                  >
                    <span className="agent-name-text">{agent.displayName}</span>
                  </button>
                  <span className="agent-row-meta">
                    <span className="agent-row-meta-label">回复</span>
                    <span className="muted-value">
                      {agent.capabilities.resume ? "支持" : "—"}
                    </span>
                  </span>
                </th>
                <td>
                  {agent.capabilities.notify ? (
                    <label className="switch-control">
                      <input
                        type="checkbox"
                        role="switch"
                        aria-label={`${agent.displayName} 通知`}
                        checked={agent.enabled}
                        disabled={pendingAgentId === agent.id}
                        onChange={(event) =>
                          onToggle(agent, event.currentTarget.checked)
                        }
                      />
                      <span className="switch-track" aria-hidden="true" />
                    </label>
                  ) : (
                    <span className="muted-value">不支持</span>
                  )}
                </td>
                <td className="agent-status-cell">
                  <StatusBadge tone={status.tone}>{status.label}</StatusBadge>
                  <span className="agent-event-cell">
                    {agent.health.detail?.message ?? "无异常"}
                  </span>
                </td>
                <td>
                  <button
                    className="icon-text-button"
                    type="button"
                    aria-label={`${agent.displayName} 配置`}
                    onClick={() => onSelect(agent.id)}
                  >
                    <Settings2 aria-hidden="true" size={15} />
                    配置
                  </button>
                </td>
              </tr>
            );
          })}
        </tbody>
      </table>
    </div>
  );
}
