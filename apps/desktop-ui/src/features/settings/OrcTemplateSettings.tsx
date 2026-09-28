import { Save } from "lucide-react";
import { useEffect, useState } from "react";

import type { HostBridge } from "../../bridge";
import type {
  AgentDto,
  OrcTemplateDto,
  OrcTemplateStepConfigDto,
} from "../../bridge/types";
import { EmptyState } from "../../components/EmptyState";
import { InlineError } from "../../components/InlineError";
import { LoadingRows } from "../../components/LoadingRows";
import { Toast } from "../../components/Toast";
import { toUserError } from "../../data/errors";
import { useSaveOrcTemplateConfigMutation } from "../../data/mutations";
import { useOrcTemplates } from "../../data/useOrcTemplates";
import { ORC_MODEL_CAPABLE_AGENT } from "../cluster/labels";
import { OrcNodeChain, type OrcNodeChainStep } from "../cluster/OrcNodeChain";

/** 保存成功提示停留时长：浮层只做结果确认，不长期挂在窗口上。 */
const SAVED_NOTICE_DURATION_MS = 3000;

/** 单个节点的本地编辑值（Agent/模型去空白后为空 = 清除该节点覆盖）。 */
interface StepDraft {
  order: number;
  agent: string;
  model: string;
}

function draftOf(template: OrcTemplateDto): StepDraft[] {
  return template.steps.map((step) => ({
    order: step.order,
    agent: step.agent?.trim() ?? "",
    model: step.model?.trim() ?? "",
  }));
}

function chainStepsOf(template: OrcTemplateDto): OrcNodeChainStep[] {
  return template.steps.map((step) => ({
    order: step.order,
    role: step.role,
    agent: step.agent,
    model: step.model,
  }));
}

export interface OrcTemplateSettingsProps {
  bridge: HostBridge;
  /** 已接入 Agent（节点下拉数据源）；为空时只能保持「未选择」。 */
  agents: AgentDto[];
}

/**
 * 设置页「工作流模板与节点配置」（§3）：三档模板选择 + 节点卡片编辑。
 * Agent 不预填（空选项显示「未选择」）；模型仅 OpenCode 可编辑，其他 Agent 置灰并注明
 * 「暂不支持指定模型」；保存走 save_orc_template_config，后端校验错误中文展示。
 */
export function OrcTemplateSettings({
  bridge,
  agents,
}: OrcTemplateSettingsProps) {
  const templatesQuery = useOrcTemplates(bridge);
  const saveMutation = useSaveOrcTemplateConfigMutation(bridge);
  const [selectedTemplateId, setSelectedTemplateId] = useState("");
  const [drafts, setDrafts] = useState<Record<string, StepDraft[]>>({});
  const [dirtyTemplates, setDirtyTemplates] = useState<Record<string, boolean>>(
    {},
  );
  const [saveError, setSaveError] = useState<unknown>(null);
  const [savedNotice, setSavedNotice] = useState<string | null>(null);

  const templates = templatesQuery.data ?? null;

  // 选中的模板：默认第一个；模板列表变化时回落到仍存在的项。
  useEffect(() => {
    if (!templates || templates.length === 0) {
      return;
    }
    setSelectedTemplateId((current) =>
      templates.some((template) => template.id === current)
        ? current
        : templates[0].id,
    );
  }, [templates]);

  // 服务端模板刷新时同步本地草稿；有未保存修改的模板保留用户输入。
  useEffect(() => {
    if (!templates) {
      return;
    }
    setDrafts((current) => {
      const next = { ...current };
      for (const template of templates) {
        if (!dirtyTemplates[template.id]) {
          next[template.id] = draftOf(template);
        }
      }
      return next;
    });
  }, [templates, dirtyTemplates]);

  useEffect(() => {
    if (!savedNotice) {
      return;
    }
    const timer = window.setTimeout(
      () => setSavedNotice(null),
      SAVED_NOTICE_DURATION_MS,
    );
    return () => window.clearTimeout(timer);
  }, [savedNotice]);

  const selectedTemplate =
    templates?.find((template) => template.id === selectedTemplateId) ?? null;
  const draft = selectedTemplate
    ? (drafts[selectedTemplate.id] ?? draftOf(selectedTemplate))
    : null;
  const dirty = selectedTemplate
    ? (dirtyTemplates[selectedTemplate.id] ?? false)
    : false;
  const saveUserError = saveError ? toUserError(saveError) : null;

  const updateStep = (order: number, patch: Partial<StepDraft>) => {
    if (!selectedTemplate) {
      return;
    }
    setSaveError(null);
    setSavedNotice(null);
    setDrafts((current) => {
      const base = current[selectedTemplate.id] ?? draftOf(selectedTemplate);
      return {
        ...current,
        [selectedTemplate.id]: base.map((step) =>
          step.order === order ? { ...step, ...patch } : step,
        ),
      };
    });
    setDirtyTemplates((current) => ({
      ...current,
      [selectedTemplate.id]: true,
    }));
  };

  const handleSave = async () => {
    if (!selectedTemplate || !draft) {
      return;
    }
    setSaveError(null);
    setSavedNotice(null);
    const steps: OrcTemplateStepConfigDto[] = draft.map((step) => ({
      order: step.order,
      agent: step.agent.trim() || null,
      model: step.model.trim() || null,
    }));
    try {
      await saveMutation.mutateAsync({
        templateId: selectedTemplate.id,
        steps,
      });
      setDirtyTemplates((current) => ({
        ...current,
        [selectedTemplate.id]: false,
      }));
      setSavedNotice("节点配置已保存");
    } catch (error) {
      setSaveError(error);
    }
  };

  const templatesError = templatesQuery.error
    ? toUserError(templatesQuery.error)
    : null;

  return (
    <div className="settings-subsection orc-template-settings">
      <div className="settings-subsection-heading">
        <strong>工作流模板与节点配置</strong>
        <small>
          选定模板后逐个节点选择 Agent 与模型，保存后作为新建任务的默认配置（创建任务时可再调整）；
          节点 Agent 不预填，模型仅 OpenCode 支持指定，留空 = 不指定（由 OpenCode 用其当前默认模型）。
        </small>
      </div>

      {templatesQuery.isPending && !templates ? (
        <LoadingRows aria-label="正在读取工作流模板" rows={2} />
      ) : null}

      {templatesError ? (
        <InlineError
          title="无法读取工作流模板"
          message={templatesError.message}
          action={
            <button
              className="button button-secondary"
              type="button"
              onClick={() => void templatesQuery.refetch()}
            >
              重新读取
            </button>
          }
        />
      ) : null}

      {templates && templates.length === 0 ? (
        <EmptyState
          title="暂无工作流模板"
          description="宿主没有返回内置模板，请确认客户端版本后重试。"
        />
      ) : null}

      {templates && templates.length > 0 && selectedTemplate && draft ? (
        <>
          <label className="orc-template-picker">
            <span>模板</span>
            <select
              className="settings-control"
              aria-label="工作流模板"
              value={selectedTemplate.id}
              disabled={saveMutation.isPending}
              onChange={(event) => {
                setSelectedTemplateId(event.currentTarget.value);
                setSaveError(null);
                setSavedNotice(null);
              }}
            >
              {templates.map((template) => (
                <option value={template.id} key={template.id}>
                  {template.name}（{template.steps.length} 步）
                </option>
              ))}
            </select>
          </label>

          {agents.length === 0 ? (
            <p className="orc-template-hint">
              当前没有已接入 Agent：节点只能保持「未选择」，请先在「Agent
              管理」页接入。
            </p>
          ) : null}

          <OrcNodeChain
            label={`${selectedTemplate.name} 节点配置`}
            steps={chainStepsOf(selectedTemplate)}
            showMeta={false}
            renderDetails={(step) => {
              const current = draft.find(
                (candidate) => candidate.order === step.order,
              );
              const agent = current?.agent ?? "";
              const model = current?.model ?? "";
              const modelDisabled = agent !== ORC_MODEL_CAPABLE_AGENT;
              return (
                <div className="orc-node-fields">
                  <label className="orc-node-field">
                    <span>Agent</span>
                    <select
                      className="settings-control"
                      aria-label={`第 ${step.order} 步 Agent`}
                      value={agent}
                      disabled={saveMutation.isPending}
                      onChange={(event) => {
                        const nextAgent = event.currentTarget.value;
                        // 切到不支持指定模型的 Agent 时清空模型，避免保存时被后端拒绝。
                        updateStep(
                          step.order,
                          nextAgent === ORC_MODEL_CAPABLE_AGENT
                            ? { agent: nextAgent }
                            : { agent: nextAgent, model: "" },
                        );
                      }}
                    >
                      <option value="">未选择</option>
                      {agents.map((candidate) => (
                        <option value={candidate.id} key={candidate.id}>
                          {candidate.displayName}（{candidate.id}）
                        </option>
                      ))}
                    </select>
                  </label>

                  <label className="orc-node-field">
                    <span>模型</span>
                    <input
                      className="settings-control"
                      aria-label={`第 ${step.order} 步 模型`}
                      type="text"
                      inputMode="text"
                      placeholder="provider/model"
                      value={model}
                      disabled={modelDisabled || saveMutation.isPending}
                      onChange={(event) =>
                        updateStep(step.order, {
                          model: event.currentTarget.value,
                        })
                      }
                    />
                  </label>

                  <p className="orc-node-field-hint">
                    {agent === ""
                      ? "请先选择 Agent"
                      : modelDisabled
                        ? "该 Agent 暂不支持指定模型"
                        : "留空 = 不指定，由 OpenCode 用其当前默认模型"}
                  </p>
                </div>
              );
            }}
          />

          {saveUserError ? (
            <InlineError
              title="节点配置保存失败"
              message={saveUserError.message}
            />
          ) : null}

          <div className="orc-template-actions">
            <button
              className="button"
              type="button"
              disabled={!dirty || saveMutation.isPending}
              onClick={() => void handleSave()}
            >
              <Save aria-hidden="true" size={15} />
              {saveMutation.isPending ? "保存中…" : "保存节点配置"}
            </button>
            <span className="orc-template-dirty">
              {dirty ? "有未保存的修改" : ""}
            </span>
          </div>
        </>
      ) : null}

      {savedNotice ? <Toast message={savedNotice} /> : null}
    </div>
  );
}
