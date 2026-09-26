import { Bot, ChevronDown, Settings2 } from "lucide-react";

import type { HostBridge } from "../../bridge";
import type { AgentDto } from "../../bridge/types";
import { StatusBadge, type StatusBadgeTone } from "../../components/patterns";
import { AgentDetail } from "./AgentDetail";

export interface AgentListProps {
  agents: AgentDto[];
  selectedAgentId: string | null;
  pendingAgentId: string | null;
  bridge: HostBridge;
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
  bridge,
  onSelect,
  onToggle,
}: AgentListProps) {
  return (
    <div className="agent-accordion-list" aria-label="Agent 列表">
      {agents.map((agent) => {
        const isExpanded = agent.id === selectedAgentId;
        const status = agentStatus(agent);

        const handleCardClick = () => {
          // 点击整张卡片：若已展开则收起（设置 null），若未展开则展开
          onSelect(isExpanded ? "" : agent.id);
        };

        return (
          <article
            key={agent.id}
            className={`agent-accordion-card ${
              isExpanded ? "agent-accordion-card--expanded" : ""
            }`}
          >
            {/* 卡片头部：整块可点（与渠道页整行可点一致）；键盘路径仍走标题按钮 */}
            <div className="agent-card-header" onClick={handleCardClick}>
              <div className="agent-card-identity">
                <span className="agent-card-icon-wrap" aria-hidden="true">
                  <Bot size={20} className="agent-card-icon" />
                </span>
                <div className="agent-card-titles">
                  <button
                    className="agent-name-button"
                    type="button"
                    aria-pressed={isExpanded}
                  >
                    <span className="agent-name-text">{agent.displayName}</span>
                  </button>
                  <div className="agent-row-meta">
                    <span className="agent-row-meta-label">回复</span>
                    <span className="muted-value">
                      {agent.capabilities.resume ? "支持" : "—"}
                    </span>
                    {agent.description && (
                      <span className="agent-card-desc-preview">
                        · {agent.description}
                      </span>
                    )}
                  </div>
                </div>
              </div>

              <div className="agent-card-controls">
                <div className="agent-status-cell">
                  <StatusBadge tone={status.tone}>{status.label}</StatusBadge>
                  <span className="agent-event-cell">
                    {agent.health.detail?.message ?? "无异常"}
                  </span>
                </div>

                <div
                  className="agent-card-switch-wrap"
                  onClick={(e) => e.stopPropagation()}
                >
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
                </div>

                <button
                  className="icon-text-button"
                  type="button"
                  aria-label={`${agent.displayName} 配置`}
                  onClick={(e) => {
                    e.stopPropagation();
                    // 点击配置确保该卡片展开
                    if (!isExpanded) {
                      onSelect(agent.id);
                    }
                  }}
                >
                  <Settings2 aria-hidden="true" size={15} />
                  配置
                </button>

                <span
                  className={`agent-card-chevron ${
                    isExpanded ? "agent-card-chevron--open" : ""
                  }`}
                  aria-hidden="true"
                >
                  <ChevronDown size={18} />
                </span>
              </div>
            </div>

            {/* 卡片下侧平滑展开内容 */}
            {isExpanded && (
              <div
                className="agent-card-body"
                id={`agent-body-${agent.id}`}
                role="region"
                aria-label={`${agent.displayName} 详细配置与能力`}
              >
                <AgentDetail agent={agent} bridge={bridge} />
              </div>
            )}
          </article>
        );
      })}
    </div>
  );
}
