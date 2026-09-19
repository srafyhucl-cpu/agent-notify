import { useMemo, useState } from "react";

import type {
  ChannelAccountDto,
  ChannelDto,
} from "../../bridge/types";
import { getSecretFieldNames, SchemaForm } from "../../components/SchemaForm";

function configRecord(value: unknown): Record<string, unknown> {
  if (typeof value === "object" && value !== null && !Array.isArray(value)) {
    return { ...value };
  }
  return {};
}

function initialSecretState(
  config: Record<string, unknown>,
  fieldNames: string[],
): Record<string, boolean> {
  return Object.fromEntries(
    fieldNames.map((name) => [
      name,
      config[name] !== undefined && config[name] !== null && config[name] !== "",
    ]),
  );
}

export interface ChannelConfigFormProps {
  channel: ChannelDto;
  account: ChannelAccountDto;
}

export function ChannelConfigForm({
  channel,
  account,
}: ChannelConfigFormProps) {
  const initialConfig = useMemo(
    () => configRecord(account.config),
    [account.config],
  );
  const [draft] = useState(initialConfig);
  const [secretValues] = useState<Record<string, string>>({});
  const secretNames = useMemo(
    () => getSecretFieldNames(channel.configSchema),
    [channel.configSchema],
  );
  const [secretConfigured] = useState(() =>
    initialSecretState(initialConfig, secretNames),
  );

  return (
    <div className="channel-config-form">
      <p className="channel-config-note">
        当前只读展示宿主返回的完整配置，不会提交局部替换。
      </p>
      <SchemaForm
        schema={channel.configSchema}
        value={draft}
        onChange={() => undefined}
        secretValues={secretValues}
        onSecretValueChange={() => undefined}
        secretConfigured={secretConfigured}
        idPrefix={`channel-config-${account.id}`}
        disabled
      />
    </div>
  );
}
