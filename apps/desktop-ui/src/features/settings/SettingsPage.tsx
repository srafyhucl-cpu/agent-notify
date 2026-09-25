import { Save } from "lucide-react";
import { useEffect, useMemo, useState } from "react";

import type { HostBridge } from "../../bridge";
import type {
  AgentDto,
  ChannelAccountDto,
  ChannelDto,
  SettingsDto,
} from "../../bridge/types";
import { EmptyState } from "../../components/EmptyState";
import { InlineError } from "../../components/InlineError";
import { LoadingRows } from "../../components/LoadingRows";
import { getSecretFieldNames, SchemaForm } from "../../components/SchemaForm";
import {
  FieldRow,
  PageHeader,
  SectionCard,
} from "../../components/patterns";
import { toUserError } from "../../data/errors";
import {
  useUpdateAgentConfigMutation,
  useUpdateSettingsMutation,
} from "../../data/mutations";
import { useAgents } from "../../data/useAgents";
import { useChannels } from "../../data/useChannels";
import { useSettings } from "../../data/useSettings";
import { QuietHoursForm } from "./QuietHoursForm";
import { ReplySettingsForm } from "./ReplySettingsForm";
import { UpdateSettings } from "./UpdateSettings";

const COOLDOWN_MIN_SECONDS = 0;
const COOLDOWN_MAX_SECONDS = 3600;
const ROUTE_TTL_MIN_SECONDS = 60;
const ROUTE_TTL_MAX_SECONDS = 604800;

interface UnavailableAction {
  name: string;
  description: string;
}

const UNAVAILABLE_DATA_ACTIONS: UnavailableAction[] = [
  {
    name: "打开数据目录",
    description: "当前版本没有稳定的打开数据目录业务命令。",
  },
  {
    name: "导出脱敏诊断包",
    description: "当前版本没有稳定的诊断包导出命令。",
  },
  {
    name: "备份数据库",
    description: "当前版本没有稳定的数据库备份命令。",
  },
];


function cloneSettings(settings: SettingsDto): SettingsDto {
  return {
    ...settings,
    quietHours: settings.quietHours ? { ...settings.quietHours } : null,
  };
}

function configRecord(value: unknown): Record<string, unknown> {
  if (typeof value === "object" && value !== null && !Array.isArray(value)) {
    return value as Record<string, unknown>;
  }
  return {};
}

function validateSettings(settings: SettingsDto): string | null {
  if (
    !Number.isInteger(settings.cooldownSeconds) ||
    settings.cooldownSeconds < COOLDOWN_MIN_SECONDS ||
    settings.cooldownSeconds > COOLDOWN_MAX_SECONDS
  ) {
    return `通知冷却必须是 ${COOLDOWN_MIN_SECONDS} 到 ${COOLDOWN_MAX_SECONDS} 之间的整数`;
  }
  if (
    !Number.isInteger(settings.routeTtlSeconds) ||
    settings.routeTtlSeconds < ROUTE_TTL_MIN_SECONDS ||
    settings.routeTtlSeconds > ROUTE_TTL_MAX_SECONDS
  ) {
    return `路由有效期必须是 ${ROUTE_TTL_MIN_SECONDS} 到 ${ROUTE_TTL_MAX_SECONDS} 之间的整数`;
  }
  if (settings.quietHours?.enabled) {
    if (!settings.quietHours.start || !settings.quietHours.end) {
      return "勿扰时段的开始和结束时间不能为空";
    }
    if (settings.quietHours.start === settings.quietHours.end) {
      return "勿扰时段的开始和结束时间不能相同";
    }
  }
  return null;
}

function settingsChanged(draft: SettingsDto, saved: SettingsDto): boolean {
  return JSON.stringify(draft) !== JSON.stringify(saved);
}

function accountChannel(
  channels: ChannelDto[],
  accountId: string | null,
): { channel: ChannelDto; account: ChannelAccountDto } | null {
  if (!accountId) {
    return null;
  }
  for (const channel of channels) {
    const account = channel.accounts.find((candidate) => candidate.id === accountId);
    if (account) {
      return { channel, account };
    }
  }
  return null;
}

export interface SettingsPageProps {
  bridge: HostBridge;
}

export function SettingsPage({ bridge }: SettingsPageProps) {
  const settingsQuery = useSettings(bridge);
  const channelsQuery = useChannels(bridge);
  const agentsQuery = useAgents(bridge);
  const updateSettingsMutation = useUpdateSettingsMutation(bridge);
  const updateAgentMutation = useUpdateAgentConfigMutation(bridge);
  const [draft, setDraft] = useState<SettingsDto | null>(null);
  const [saveError, setSaveError] = useState<unknown>(null);
  const [validationError, setValidationError] = useState<string | null>(null);
  const [savedMessage, setSavedMessage] = useState<string | null>(null);
  const [agentActionError, setAgentActionError] = useState<unknown>(null);
  const [pendingAgentId, setPendingAgentId] = useState<string | null>(null);

  useEffect(() => {
    if (settingsQuery.data) {
      setDraft(cloneSettings(settingsQuery.data));
      setValidationError(null);
    }
  }, [settingsQuery.data]);

  const channels = channelsQuery.data?.channels ?? [];
  const agents = agentsQuery.data ?? [];
  const selectedChannelAccount = accountChannel(
    channels,
    draft?.defaultChannelAccountId ?? null,
  );
  const channelSecretNames = useMemo(
    () => getSecretFieldNames(selectedChannelAccount?.channel.configSchema),
    [selectedChannelAccount],
  );
  const channelSecretConfigured = useMemo(() => {
    const config = configRecord(selectedChannelAccount?.account.config);
    return Object.fromEntries(
      channelSecretNames.map((name) => [
        name,
        config[name] !== undefined && config[name] !== null && config[name] !== "",
      ]),
    );
  }, [channelSecretNames, selectedChannelAccount]);
  const loadError = settingsQuery.error ? toUserError(settingsQuery.error) : null;
  const saveUserError = saveError ? toUserError(saveError) : null;
  const agentUserError = agentActionError ? toUserError(agentActionError) : null;
  const canSave =
    draft !== null &&
    settingsQuery.data !== undefined &&
    settingsChanged(draft, settingsQuery.data) &&
    !updateSettingsMutation.isPending;

  const patchDraft = (patch: Partial<SettingsDto>) => {
    setSavedMessage(null);
    setSaveError(null);
    setDraft((current) => (current ? { ...current, ...patch } : current));
  };

  const submit = async () => {
    if (!draft) {
      return;
    }
    setSavedMessage(null);
    setSaveError(null);
    const validation = validateSettings(draft);
    setValidationError(validation);
    if (validation) {
      return;
    }

    try {
      const updated = await updateSettingsMutation.mutateAsync(draft);
      const next = cloneSettings(updated);
      setDraft(next);
      setSavedMessage("设置已保存");
    } catch (error) {
      setSaveError(error);
      if (settingsQuery.data) {
        setDraft(cloneSettings(settingsQuery.data));
      }
    }
  };

  const toggleAgent = async (agent: AgentDto, enabled: boolean) => {
    setAgentActionError(null);
    setPendingAgentId(agent.id);
    try {
      await updateAgentMutation.mutateAsync({
        agentId: agent.id,
        enabled,
        config: null,
      });
    } catch (error) {
      setAgentActionError(error);
    } finally {
      setPendingAgentId(null);
    }
  };

  return (
    <section className="workbench-page settings-page">
      <PageHeader
        title="设置"
        summary="设置按通知、回复、渠道、应用和数据分组；未提供稳定命令的操作保持不可用。"
        actions={
          <button
            className="button"
            type="button"
            disabled={!canSave}
            onClick={() => void submit()}
            aria-label="保存设置（顶部）"
          >
            <Save aria-hidden="true" size={15} />
            {updateSettingsMutation.isPending ? "正在保存" : "保存设置"}
          </button>
        }
      />

      <div className="workbench-page-content settings-page-content">
        {loadError ? (
          <InlineError
            title="无法读取设置"
            message={loadError.message}
            action={
              <button
                className="button button-secondary"
                type="button"
                onClick={() => void settingsQuery.refetch()}
              >
                重新检查
              </button>
            }
          />
        ) : null}

        {saveUserError ? (
          <InlineError title={saveUserError.title} message={saveUserError.message} />
        ) : null}

        {validationError ? (
          <p className="settings-feedback settings-feedback--error" role="alert">
            {validationError}
          </p>
        ) : null}

        {savedMessage ? (
          <p className="settings-feedback settings-feedback--success" role="status">
            {savedMessage}
          </p>
        ) : null}

        {settingsQuery.isPending && !draft ? (
          <LoadingRows aria-label="正在读取设置" rows={7} />
        ) : null}

        {!settingsQuery.isPending && !draft && !loadError ? (
          <EmptyState
            title="暂无设置"
            description="宿主尚未返回可显示的非敏感设置。"
          />
        ) : null}

        {draft ? (
          <div className="settings-container">
            <form
              className="settings-form"
              aria-label="AgentNotify 设置"
              aria-busy={updateSettingsMutation.isPending}
              noValidate
              onSubmit={(event) => {
                event.preventDefault();
                void submit();
              }}
            >
              <div className="settings-panels">
                <div id="settings-notifications" className="settings-anchor">
                  <SectionCard
                    title="通知"
                    description="控制通知暂停、勿扰时段与 Agent 默认接入。"
                  >
                    <div className="settings-fields">
                      <FieldRow
                        label="全局暂停"
                        description="保存后暂停新的通知调度"
                        control={
                          <input
                            type="checkbox"
                            role="switch"
                            aria-label="全局暂停"
                            checked={draft.notificationsPaused}
                            disabled={updateSettingsMutation.isPending}
                            onChange={(event) =>
                              patchDraft({
                                notificationsPaused: event.currentTarget.checked,
                              })
                            }
                          />
                        }
                      />

                      <QuietHoursForm
                        value={draft.quietHours}
                        disabled={updateSettingsMutation.isPending}
                        onChange={(quietHours) => patchDraft({ quietHours })}
                      />

                      <FieldRow
                        label="通知冷却（秒）"
                        description="允许 0 到 3600 秒。"
                        control={
                          <input
                            className="settings-control"
                            aria-label="通知冷却（秒）"
                            type="number"
                            min={COOLDOWN_MIN_SECONDS}
                            max={COOLDOWN_MAX_SECONDS}
                            step={1}
                            value={draft.cooldownSeconds}
                            disabled={updateSettingsMutation.isPending}
                            onChange={(event) =>
                              patchDraft({
                                cooldownSeconds:
                                  Number.parseInt(event.currentTarget.value, 10) || 0,
                              })
                            }
                          />
                        }
                      />

                      <div className="settings-subsection">
                        <div className="settings-subsection-heading">
                          <strong>Agent 默认开关</strong>
                          <small>修改后立即保存，不等待页面保存按钮。</small>
                        </div>
                        {agentUserError ? (
                          <InlineError
                            title={agentUserError.title}
                            message={agentUserError.message}
                          />
                        ) : null}
                        {agents.length === 0 ? (
                          <p className="section-empty">当前没有已接入 Agent。</p>
                        ) : (
                          <div className="settings-agent-list">
                            {agents.map((agent) => (
                              <FieldRow
                                key={agent.id}
                                label={agent.displayName}
                                description={agent.id}
                                control={
                                  <input
                                    type="checkbox"
                                    role="switch"
                                    aria-label={`${agent.displayName} 默认通知`}
                                    checked={agent.enabled}
                                    disabled={
                                      pendingAgentId === agent.id ||
                                      !agent.capabilities.notify
                                    }
                                    onChange={(event) =>
                                      void toggleAgent(agent, event.currentTarget.checked)
                                    }
                                  />
                                }
                              />
                            ))}
                          </div>
                        )}
                      </div>
                    </div>
                  </SectionCard>
                </div>

                <div id="settings-replies" className="settings-anchor">
                  <SectionCard
                    title="回复"
                    description="控制引用回复、送达确认和路由有效期。"
                  >
                    <ReplySettingsForm
                      value={draft}
                      disabled={updateSettingsMutation.isPending}
                      onChange={patchDraft}
                    />
                  </SectionCard>
                </div>

                <div id="settings-channels" className="settings-anchor">
                  <SectionCard
                    title="渠道"
                    description="默认通知账号必须显示具体账号 ID。"
                  >
                    <div className="settings-fields">
                      <FieldRow
                        label="默认通知账号"
                        description="选择默认用于发送通知的渠道账号。"
                        control={
                          <select
                            className="settings-control"
                            aria-label="默认通知账号"
                            value={draft.defaultChannelAccountId ?? ""}
                            disabled={updateSettingsMutation.isPending}
                            onChange={(event) =>
                              patchDraft({
                                defaultChannelAccountId:
                                  event.currentTarget.value || null,
                              })
                            }
                          >
                            <option value="">未选择</option>
                            {channels.flatMap((channel) =>
                              channel.accounts.map((account) => (
                                <option value={account.id} key={account.id}>
                                  {channel.displayName} / {account.displayName}（
                                  {account.id}）
                                </option>
                              )),
                            )}
                          </select>
                        }
                      />

                      <div className="settings-unavailable-row">
                        <span>
                          <strong>渠道连接配置更新</strong>
                          <small>当前版本不可用，请到渠道页面登录或管理账号。</small>
                        </span>
                        <span className="settings-unavailable-label">当前版本不可用</span>
                      </div>

                      {selectedChannelAccount ? (
                        <div className="settings-readonly-config">
                          <div className="settings-subsection-heading">
                            <strong>账号配置只读预览</strong>
                            <small>
                              {selectedChannelAccount.channel.displayName} /{" "}
                              {selectedChannelAccount.account.id}
                            </small>
                          </div>
                          <SchemaForm
                            schema={selectedChannelAccount.channel.configSchema}
                            value={configRecord(selectedChannelAccount.account.config)}
                            onChange={() => undefined}
                            secretValues={{}}
                            onSecretValueChange={() => undefined}
                            secretConfigured={channelSecretConfigured}
                            idPrefix={`settings-channel-${selectedChannelAccount.account.id}`}
                            disabled
                          />
                        </div>
                      ) : null}
                    </div>
                  </SectionCard>
                </div>

                <div id="settings-application" className="settings-anchor">
                  <SectionCard
                    title="应用"
                    description="控制启动行为和更新通道。"
                  >
                    <UpdateSettings
                      bridge={bridge}
                      value={draft}
                      disabled={updateSettingsMutation.isPending}
                      onChange={patchDraft}
                    />
                  </SectionCard>
                </div>

                <div id="settings-data" className="settings-anchor">
                  <SectionCard
                    title="数据"
                    description="数据操作仅在宿主提供稳定业务命令后启用。"
                  >
                    <div className="settings-data-actions">
                      {UNAVAILABLE_DATA_ACTIONS.map((action) => (
                        <div className="settings-unavailable-row" key={action.name}>
                          <span>
                            <strong>{action.name}</strong>
                            <small>{action.description}</small>
                          </span>
                          <button
                            className="button button-secondary"
                            type="button"
                            disabled
                            aria-label={action.name}
                          >
                            当前版本不可用
                          </button>
                        </div>
                      ))}
                    </div>
                  </SectionCard>
                </div>
              </div>

              <div className="settings-save-row">
                <span>
                  {canSave ? "有尚未保存的修改" : "所有已支持设置均已保存"}
                </span>
                <button className="button" type="submit" disabled={!canSave}>
                  <Save aria-hidden="true" size={15} />
                  {updateSettingsMutation.isPending ? "正在保存" : "保存设置"}
                </button>
              </div>
            </form>
          </div>
        ) : null}
      </div>
    </section>
  );
}
