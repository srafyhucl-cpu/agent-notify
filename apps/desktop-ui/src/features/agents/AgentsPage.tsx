import { Bot } from "lucide-react";
import { useState } from "react";

import type { HostBridge } from "../../bridge";
import type { AgentDto } from "../../bridge/types";
import { EmptyState } from "../../components/EmptyState";
import { InlineError } from "../../components/InlineError";
import { LoadingRows } from "../../components/LoadingRows";
import { PageHeader } from "../../components/patterns";
import { toUserError } from "../../data/errors";
import { useUpdateAgentConfigMutation } from "../../data/mutations";
import { useAgents } from "../../data/useAgents";
import { AgentList } from "./AgentList";

export interface AgentsPageProps {
  bridge: HostBridge;
}

export function AgentsPage({ bridge }: AgentsPageProps) {
  const agentsQuery = useAgents(bridge);
  const updateMutation = useUpdateAgentConfigMutation(bridge);
  const [selectedAgentId, setSelectedAgentId] = useState<string | null>(null);
  const [pendingAgentId, setPendingAgentId] = useState<string | null>(null);
  const [actionError, setActionError] = useState<unknown>(null);

  const agents = agentsQuery.data ?? [];
  const selectedAgent =
    agents.find((agent) => agent.id === selectedAgentId) ?? agents[0] ?? null;
  const loadError = agentsQuery.error ? toUserError(agentsQuery.error) : null;
  const updateError = actionError ? toUserError(actionError) : null;

  const toggleAgent = async (agent: AgentDto, enabled: boolean) => {
    setActionError(null);
    setPendingAgentId(agent.id);
    try {
      await updateMutation.mutateAsync({
        agentId: agent.id,
        enabled,
        config: null,
      });
    } catch (error) {
      setActionError(error);
    } finally {
      setPendingAgentId(null);
    }
  };

  return (
    <section className="workbench-page agents-page" aria-label="Agent 管理">
      <PageHeader
        title="Agent 管理"
        summary="按 descriptor 展示已接入 Agent 的能力、状态与配置。"
        actions={
          <span className="page-count" aria-label={`共 ${agents.length} 个 Agent`}>
            <Bot aria-hidden="true" size={16} />
            {agents.length} 个
          </span>
        }
      />

      <div className="workbench-page-content agents-page-content">
        {loadError ? (
          <InlineError
            title="无法读取 Agent 列表"
            message={loadError.message}
            action={
              <button
                className="button button-secondary"
                type="button"
                onClick={() => void agentsQuery.refetch()}
              >
                重新检查
              </button>
            }
          />
        ) : null}

        {updateError ? (
          <InlineError title={updateError.title} message={updateError.message} />
        ) : null}

        {agentsQuery.isPending && !agentsQuery.data ? (
          <LoadingRows aria-label="正在加载 Agent 列表" />
        ) : null}

        {!agentsQuery.isPending && agents.length === 0 && !loadError ? (
          <EmptyState
            title="暂无 Agent"
            description="当前没有已接入的 Agent。安装或启用 Agent 适配器后，这里会自动显示。"
          />
        ) : null}

        {agents.length > 0 ? (
          <div className="agents-container">
            <AgentList
              agents={agents}
              selectedAgentId={
                selectedAgentId === ""
                  ? null
                  : (selectedAgentId ?? agents[0]?.id ?? null)
              }
              pendingAgentId={pendingAgentId}
              bridge={bridge}
              onSelect={(id) => setSelectedAgentId(id)}
              onToggle={(agent, enabled) => void toggleAgent(agent, enabled)}
            />
          </div>
        ) : null}
      </div>
    </section>
  );
}
