import { Settings2 } from "lucide-react";

import type { AgentDto } from "../../bridge/types";

export interface AgentListProps {
  agents: AgentDto[];
  selectedAgentId: string | null;
  pendingAgentId: string | null;
  onSelect: (agentId: string) => void;
  onToggle: (agent: AgentDto, enabled: boolean) => void;
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
            <th scope="col">回复</th>
            <th scope="col">接入状态</th>
            <th scope="col">最近事件</th>
            <th scope="col">配置</th>
          </tr>
        </thead>
        <tbody>
          {agents.map((agent) => (
            <tr
              className={
                agent.id === selectedAgentId ? "agent-row--selected" : undefined
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
                  {agent.displayName}
                </button>
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
              <td>
                <span className="muted-value">
                  {agent.capabilities.resume ? "支持" : "—"}
                </span>
              </td>
              <td>
                <span
                  className={`status-label ${
                    agent.health.available
                      ? "status-label--success"
                      : "status-label--danger"
                  }`}
                >
                  {agent.health.available ? "接入正常" : "接入异常"}
                </span>
              </td>
              <td className="agent-event-cell">
                {agent.health.detail?.message ?? "无异常"}
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
          ))}
        </tbody>
      </table>
    </div>
  );
}
