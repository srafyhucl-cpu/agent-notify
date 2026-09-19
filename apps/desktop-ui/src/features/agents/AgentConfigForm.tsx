import { useMemo, useState } from "react";

import type { HostBridge } from "../../bridge";
import type { AgentDto } from "../../bridge/types";
import { InlineError } from "../../components/InlineError";
import { getSecretFieldNames, SchemaForm } from "../../components/SchemaForm";
import { toUserError } from "../../data/errors";
import { useUpdateAgentConfigMutation } from "../../data/mutations";

function configRecord(value: unknown): Record<string, unknown> {
  if (typeof value === "object" && value !== null && !Array.isArray(value)) {
    return { ...value };
  }
  return {};
}

function initialSecretState(
  config: Record<string, unknown>,
  secretNames: string[],
): Record<string, boolean> {
  return Object.fromEntries(
    secretNames.map((name) => [
      name,
      config[name] !== undefined && config[name] !== null && config[name] !== "",
    ]),
  );
}

export interface AgentConfigFormProps {
  agent: AgentDto;
  bridge: HostBridge;
}

export function AgentConfigForm({ agent, bridge }: AgentConfigFormProps) {
  const secretNames = useMemo(
    () => getSecretFieldNames(agent.configSchema),
    [agent.configSchema],
  );
  const initialConfig = configRecord(agent.config);
  const [draft, setDraft] = useState(initialConfig);
  const [secretValues, setSecretValues] = useState<Record<string, string>>({});
  const [secretConfigured, setSecretConfigured] = useState(() =>
    initialSecretState(initialConfig, secretNames),
  );
  const [submitError, setSubmitError] = useState<unknown>(null);
  const mutation = useUpdateAgentConfigMutation(bridge);

  const submit = async () => {
    setSubmitError(null);

    const config = { ...draft };
    for (const name of secretNames) {
      delete config[name];
    }
    for (const [name, value] of Object.entries(secretValues)) {
      if (value !== "") {
        config[name] = value;
      }
    }

    try {
      const updated = await mutation.mutateAsync({
        agentId: agent.id,
        enabled: null,
        config,
      });
      const nextDraft = configRecord(updated.config);
      for (const name of secretNames) {
        delete nextDraft[name];
      }
      setDraft(nextDraft);
      setSecretValues({});
      setSecretConfigured((current) => {
        const next = { ...current };
        for (const [name, value] of Object.entries(secretValues)) {
          if (value !== "") {
            next[name] = true;
          }
        }
        return next;
      });
    } catch (error) {
      setSubmitError(error);
    }
  };

  const userError = submitError ? toUserError(submitError) : null;

  return (
    <form
      className="agent-config-form"
      aria-label={`${agent.displayName} 配置`}
      aria-busy={mutation.isPending}
      onSubmit={(event) => {
        event.preventDefault();
        void submit();
      }}
    >
      <SchemaForm
        schema={agent.configSchema}
        value={draft}
        onChange={setDraft}
        secretValues={secretValues}
        onSecretValueChange={(name, value) =>
          setSecretValues((current) => ({ ...current, [name]: value }))
        }
        secretConfigured={secretConfigured}
        idPrefix={`agent-config-${agent.id}`}
        disabled={mutation.isPending}
      />

      {userError ? (
        <InlineError title={userError.title} message={userError.message} />
      ) : null}

      <div className="agent-config-actions">
        <button className="button" type="submit" disabled={mutation.isPending}>
          {mutation.isPending ? "正在保存" : "保存配置"}
        </button>
      </div>
    </form>
  );
}
