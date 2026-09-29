import { X } from "lucide-react";
import { useEffect, useRef, useState, type MouseEvent } from "react";

import type { HostBridge } from "../../bridge";
import type {
  OpencodeProjectDto,
  OrcTemplateDto,
  OrcTemplateStepConfigDto,
} from "../../bridge/types";
import { InlineError } from "../../components/InlineError";
import { toUserError } from "../../data/errors";
import { useAgents } from "../../data/useAgents";
import { useOpencodeModels } from "../../data/useOpencodeModels";
import { ORC_MODEL_CAPABLE_AGENT, ORC_NOTIFY_MODE_OPTIONS, TASK_NAME_MAX_CHARS } from "./labels";
import { OrcModelSelect } from "./OrcModelSelect";
import { OrcNodeChain, type OrcNodeChainStep } from "./OrcNodeChain";

/** 工作目录下拉里的「手动输入」哨兵值（目录不会与它同名）。 */
const MANUAL_WORK_DIR = "__manual__";

/** 单个节点的本地编辑值（提交时全量带上，创建即锁定进任务）。 */
interface StepDraft {
  order: number;
  agent: string;
  model: string;
}

/** 创建任务提交内容（§3.1：名称必填 ≤8 字、模板必选、工作目录必填、每个节点必须有 Agent）。 */
export interface CreateOrcTaskInput {
  /** 任务名称（≤8 字；只用于集群列表与真实会话标题展示）。 */
  name: string;
  /** 任务描述（真实需求全文）。 */
  goal: string;
  notifyMode: string;
  templateId: string;
  workingDir: string;
  /** 任务级节点配置：创建即快照锁定，此后改设置不影响该任务。 */
  steps: OrcTemplateStepConfigDto[];
}

export interface CreateTaskDialogProps {
  bridge: HostBridge;
  /** 创建请求进行中：禁用提交并展示按钮内加载态。 */
  pending: boolean;
  /** 可选模板（null = 尚未取到）；读取失败见 templatesError。 */
  templates: OrcTemplateDto[] | null;
  /** 模板读取失败原因（已转成用户可读文案）；非空时禁止提交。 */
  templatesError: string | null;
  onRetryTemplates: () => void;
  /** OpenCode 已知项目（null = 尚未取到）；读取失败见 projectsError。 */
  projects: OpencodeProjectDto[] | null;
  /** 项目列表读取失败原因；显示明确错误并退回手动输入（不静默）。 */
  projectsError: string | null;
  onRetryProjects: () => void;
  /** 创建失败原因（已转成用户可读文案）；对话框内展示，不关闭。 */
  error: unknown;
  /** 提交创建（startImmediately = 创建并开始）；返回是否已创建成功。 */
  onSubmit: (
    input: CreateOrcTaskInput,
    startImmediately: boolean,
  ) => Promise<boolean>;
  onClose: () => void;
}

function chainStepsOf(template: OrcTemplateDto): OrcNodeChainStep[] {
  return template.steps.map((step) => ({
    order: step.order,
    role: step.role,
    agent: step.agent,
    model: step.model,
  }));
}

/**
 * 新建编排任务（弹窗，对齐渠道页「添加账号」）：
 * 目标 + 通知节奏 + 模板 + 工作目录 + 逐个节点选择 Agent / 手填模型。
 * 节点起点 = 设置页保存的默认配置（未配置则为空，不预填）；提交后创建即锁定。
 */
export function CreateTaskDialog({
  bridge,
  pending,
  templates,
  templatesError,
  onRetryTemplates,
  projects,
  projectsError,
  onRetryProjects,
  error,
  onSubmit,
  onClose,
}: CreateTaskDialogProps) {
  const agentsQuery = useAgents(bridge);
  const agents = agentsQuery.data ?? [];
  const modelsQuery = useOpencodeModels(bridge);
  const modelsError = modelsQuery.error
    ? toUserError(modelsQuery.error).message
    : null;
  const [name, setName] = useState("");
  const [goal, setGoal] = useState("");
  const [notifyMode, setNotifyMode] = useState("final_only");
  const [templateId, setTemplateId] = useState("");
  const [projectSelection, setProjectSelection] = useState("");
  const [manualDir, setManualDir] = useState("");
  const [drafts, setDrafts] = useState<StepDraft[]>([]);
  const closeButtonRef = useRef<HTMLButtonElement>(null);
  const userError = error ? toUserError(error) : null;

  const selectedTemplate =
    templates?.find((template) => template.id === templateId) ?? null;

  // 选模板后用模板当前配置（设置页默认合并结果）作为节点起点；节点可逐个改。
  useEffect(() => {
    if (!selectedTemplate) {
      setDrafts([]);
      return;
    }
    setDrafts(
      selectedTemplate.steps.map((step) => ({
        order: step.order,
        agent: step.agent?.trim() ?? "",
        model: step.model?.trim() ?? "",
      })),
    );
  }, [selectedTemplate]);

  useEffect(() => {
    closeButtonRef.current?.focus();
    const handleKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        onClose();
      }
    };
    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, [onClose]);

  const handleOverlayMouseDown = (event: MouseEvent<HTMLDivElement>) => {
    if (event.target === event.currentTarget) {
      onClose();
    }
  };

  const updateStep = (order: number, patch: Partial<StepDraft>) => {
    setDrafts((current) =>
      current.map((step) =>
        step.order === order ? { ...step, ...patch } : step,
      ),
    );
  };

  const projectOptions = projects ?? [];
  // 项目列表不可用（读取失败）或为空时，退回手动输入（不静默使用空选择）。
  const projectsUnavailable = projectsError !== null;
  const noProjects = projects !== null && projects.length === 0;
  const manualMode =
    projectsUnavailable || noProjects || projectSelection === MANUAL_WORK_DIR;
  const workingDir = manualMode ? manualDir.trim() : projectSelection;
  const selectValue =
    projectsUnavailable || noProjects ? MANUAL_WORK_DIR : projectSelection;
  const missingAgentOrders = drafts
    .filter((step) => step.agent.trim().length === 0)
    .map((step) => step.order);
  const trimmedName = name.trim();
  const nameOk =
    trimmedName.length > 0 && [...trimmedName].length <= TASK_NAME_MAX_CHARS;
  const canSubmit =
    nameOk &&
    goal.trim().length > 0 &&
    templateId.length > 0 &&
    workingDir.length > 0 &&
    drafts.length > 0 &&
    missingAgentOrders.length === 0 &&
    templatesError === null &&
    !pending;

  const submit = async (startImmediately: boolean) => {
    if (!canSubmit) {
      return;
    }
    await onSubmit(
      {
        name: trimmedName,
        goal: goal.trim(),
        notifyMode,
        templateId,
        workingDir,
        steps: drafts.map((step) => ({
          order: step.order,
          agent: step.agent.trim() || null,
          model: step.model.trim() || null,
        })),
      },
      startImmediately,
    );
  };

  return (
    <div
      className="dialog-overlay"
      role="presentation"
      onMouseDown={handleOverlayMouseDown}
    >
      <section
        className="cluster-create-dialog"
        role="dialog"
        aria-modal="true"
        aria-labelledby="cluster-create-title"
        aria-busy={pending}
      >
        <header className="dialog-header">
          <div>
            <h2 id="cluster-create-title">新建编排任务</h2>
            <p className="dialog-subtitle">
              选定模板后为每个节点选择 Agent 与模型；创建即锁定，之后改设置不影响它。
            </p>
          </div>
          <button
            ref={closeButtonRef}
            className="icon-button"
            type="button"
            aria-label="关闭新建任务窗口"
            onClick={onClose}
          >
            <X aria-hidden="true" size={18} />
          </button>
        </header>

        <div className="cluster-dialog-body">
          {userError ? (
            <InlineError title={userError.title} message={userError.message} />
          ) : null}

          {templatesError ? (
            <InlineError
              title="无法读取工作流模板"
              message={templatesError}
              action={
                <button
                  className="button button-secondary"
                  type="button"
                  onClick={onRetryTemplates}
                >
                  重新读取
                </button>
              }
            />
          ) : null}

          <div className="cluster-create-fields cluster-create-fields--trio">
            <label>
              <span>任务名称</span>
              <input
                aria-label="任务名称"
                className="cluster-create-name"
                type="text"
                maxLength={TASK_NAME_MAX_CHARS}
                placeholder="8 个字以内"
                value={name}
                onChange={(event) => setName(event.currentTarget.value)}
              />
            </label>

            <label>
              <span>通知节奏</span>
              <select
                aria-label="通知节奏"
                value={notifyMode}
                onChange={(event) => setNotifyMode(event.currentTarget.value)}
              >
                {ORC_NOTIFY_MODE_OPTIONS.map((option) => (
                  <option value={option.value} key={option.value}>
                    {option.label}
                  </option>
                ))}
              </select>
            </label>

            <label>
              <span>工作流模板</span>
              <select
                aria-label="工作流模板"
                value={templateId}
                disabled={templatesError !== null}
                onChange={(event) => setTemplateId(event.currentTarget.value)}
              >
                <option value="">请选择模板</option>
                {(templates ?? []).map((template) => (
                  <option value={template.id} key={template.id}>
                    {template.name}（{template.steps.length} 步）
                  </option>
                ))}
              </select>
            </label>
          </div>

          <label className="cluster-create-goal-field">
            <span>任务描述</span>
            <textarea
              aria-label="任务描述"
              className="cluster-create-goal"
              rows={3}
              placeholder="写清真实需求：做什么、交付什么、有什么约束"
              value={goal}
              onChange={(event) => setGoal(event.currentTarget.value)}
            />
          </label>

          <div className="cluster-create-dir-field">
            <label>
              <span>工作目录</span>
              <select
                aria-label="工作目录"
                value={selectValue}
                onChange={(event) =>
                  setProjectSelection(event.currentTarget.value)
                }
              >
                {projects === null && projectsError === null ? (
                  <option value="">正在读取 OpenCode 项目…</option>
                ) : null}
                {projectOptions.length > 0 ? (
                  <option value="">请选择工作目录</option>
                ) : null}
                {projectOptions.map((project) => (
                  <option value={project.directory} key={project.directory}>
                    {project.name
                      ? `${project.name}（${project.directory}）`
                      : project.directory}
                  </option>
                ))}
                <option value={MANUAL_WORK_DIR}>手动输入</option>
              </select>
            </label>

            {manualMode ? (
              <label>
                <span className="cluster-create-dir-manual-label">
                  手动输入目录
                </span>
                <input
                  aria-label="工作目录（手动输入）"
                  className="cluster-create-dir-input"
                  type="text"
                  placeholder="例如：D:/Project/my-app"
                  value={manualDir}
                  onChange={(event) => setManualDir(event.currentTarget.value)}
                />
              </label>
            ) : null}

            {projectsError ? (
              <InlineError
                title="无法读取 OpenCode 项目"
                message={projectsError}
                action={
                  <button
                    className="button button-secondary"
                    type="button"
                    onClick={onRetryProjects}
                  >
                    重新读取
                  </button>
                }
              />
            ) : null}

            {noProjects ? (
              <p className="cluster-create-dir-hint">
                未读取到 OpenCode 项目，请手动输入工作目录。
              </p>
            ) : null}
          </div>

          {selectedTemplate ? (
            <div className="cluster-workflow-preview">
              <p className="cluster-workflow-preview-title">
                {selectedTemplate.name} · 工作流节点（第 1 步是项目经理，负责汇总汇报）
              </p>
              {modelsError ? (
                <InlineError
                  title="无法读取 OpenCode 模型列表"
                  message={modelsError}
                  action={
                    <button
                      className="button button-secondary"
                      type="button"
                      onClick={() => void modelsQuery.refetch()}
                    >
                      重新读取
                    </button>
                  }
                />
              ) : null}
              <OrcNodeChain
                label={`${selectedTemplate.name} 节点配置`}
                steps={chainStepsOf(selectedTemplate)}
                showMeta={false}
                layout="grid"
                renderDetails={(step) => {
                  const current = drafts.find(
                    (candidate) => candidate.order === step.order,
                  );
                  const agent = current?.agent ?? "";
                  const model = current?.model ?? "";
                  const modelDisabled = agent !== ORC_MODEL_CAPABLE_AGENT;
                  return (
                    <div className="orc-node-fields orc-node-fields--pair">
                      <label className="orc-node-field">
                        <span>Agent</span>
                        <select
                          aria-label={`第 ${step.order} 步 Agent`}
                          value={agent}
                          disabled={pending}
                          onChange={(event) => {
                            const nextAgent = event.currentTarget.value;
                            // 切到不支持指定模型的 Agent 时清空模型，避免提交时被拒绝。
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
                        <OrcModelSelect
                          ariaLabel={`第 ${step.order} 步 模型`}
                          value={model}
                          onChange={(next) =>
                            updateStep(step.order, { model: next })
                          }
                          editable={!modelDisabled}
                          disabled={pending}
                          models={modelsQuery.data ?? null}
                          error={modelsError}
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
              {agents.length === 0 ? (
                <p className="cluster-create-dir-hint">
                  当前没有已接入 Agent：请先在「Agent 管理」页接入后重试。
                </p>
              ) : null}
              {missingAgentOrders.length > 0 ? (
                <p className="cluster-create-agent-hint">
                  第 {missingAgentOrders.join("、")} 步未选择 Agent：全部选好后才能创建。
                </p>
              ) : null}
            </div>
          ) : null}
        </div>

        <div className="cluster-dialog-actions">
          <button
            className="button button-secondary"
            type="button"
            disabled={pending}
            onClick={onClose}
          >
            取消
          </button>
          <button
            className="button button-secondary"
            type="button"
            disabled={!canSubmit}
            onClick={() => void submit(false)}
          >
            仅创建
          </button>
          <button
            className="button"
            type="button"
            disabled={!canSubmit}
            onClick={() => void submit(true)}
          >
            {pending ? "创建中…" : "创建并开始"}
          </button>
        </div>
      </section>
    </div>
  );
}
