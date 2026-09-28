import { Plus } from "lucide-react";
import { useState, type FormEvent } from "react";

import type { OpencodeProjectDto, OrcTemplateDto } from "../../bridge/types";
import { InlineError } from "../../components/InlineError";
import { SectionCard } from "../../components/patterns";
import { ORC_NOTIFY_MODE_OPTIONS } from "./labels";
import { OrcNodeChain, type OrcNodeChainStep } from "./OrcNodeChain";

/** 工作目录下拉里的「手动输入」哨兵值（目录不会与它同名）。 */
const MANUAL_WORK_DIR = "__manual__";

/** 创建任务提交内容（§3.1：模板必选、工作目录必填）。 */
export interface CreateOrcTaskInput {
  goal: string;
  notifyMode: string;
  templateId: string;
  workingDir: string;
}

export interface CreateTaskFormProps {
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
  /** 提交创建请求（含模板与工作目录）；返回成功与否，成功才清空表单。 */
  onSubmit: (input: CreateOrcTaskInput) => Promise<boolean>;
}

function toNodeChainSteps(template: OrcTemplateDto): OrcNodeChainStep[] {
  return template.steps.map((step) => ({
    order: step.order,
    role: step.role,
    agent: step.agent,
    model: step.model,
  }));
}

/**
 * 创建编排任务（§3.1）：目标 + 通知节奏 + 模板（必选）+ 工作目录（必填）。
 * 选模板后即时预览节点链（用途/Agent/模型，首节点标项目经理）；工作目录支持
 * OpenCode 项目下拉与手动输入，项目列表读取失败时明确报错并退回手动输入。
 */
export function CreateTaskForm({
  pending,
  templates,
  templatesError,
  onRetryTemplates,
  projects,
  projectsError,
  onRetryProjects,
  onSubmit,
}: CreateTaskFormProps) {
  const [goal, setGoal] = useState("");
  const [notifyMode, setNotifyMode] = useState("final_only");
  const [templateId, setTemplateId] = useState("");
  const [projectSelection, setProjectSelection] = useState("");
  const [manualDir, setManualDir] = useState("");

  const projectOptions = projects ?? [];
  const selectedTemplate =
    templates?.find((template) => template.id === templateId) ?? null;
  // 项目列表不可用（读取失败）或为空时，退回手动输入（不静默使用空选择）。
  const projectsUnavailable = projectsError !== null;
  const noProjects = projects !== null && projects.length === 0;
  const manualMode =
    projectsUnavailable ||
    noProjects ||
    projectSelection === MANUAL_WORK_DIR;
  const workingDir = manualMode ? manualDir.trim() : projectSelection;
  const selectValue =
    projectsUnavailable || noProjects ? MANUAL_WORK_DIR : projectSelection;
  const canSubmit =
    goal.trim().length > 0 &&
    templateId.length > 0 &&
    workingDir.length > 0 &&
    templatesError === null &&
    !pending;

  const handleSubmit = async (event: FormEvent) => {
    event.preventDefault();
    if (!canSubmit) {
      return;
    }
    const created = await onSubmit({
      goal: goal.trim(),
      notifyMode,
      templateId,
      workingDir,
    });
    if (created) {
      setGoal("");
      setNotifyMode("final_only");
      setTemplateId("");
      setProjectSelection("");
      setManualDir("");
    }
  };

  return (
    <SectionCard
      title="创建任务"
      description="选择工作流模板与工作目录，确认节点后创建；创建后先点「开始执行」才会派活第 1 步。"
    >
      <form
        className="cluster-create-form"
        aria-label="创建集群任务"
        onSubmit={handleSubmit}
      >
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

        {selectedTemplate ? (
          <div className="cluster-workflow-preview">
            <p className="cluster-workflow-preview-title">
              {selectedTemplate.name}（{selectedTemplate.steps.length} 步）
            </p>
            <OrcNodeChain
              label="工作流节点预览"
              steps={toNodeChainSteps(selectedTemplate)}
            />
          </div>
        ) : null}

        <div className="cluster-create-fields">
          <label className="cluster-create-goal-field">
            <span>目标</span>
            <textarea
              aria-label="目标"
              className="cluster-create-goal"
              rows={2}
              placeholder="例如：把登录流程加入重试机制"
              value={goal}
              onChange={(event) => setGoal(event.currentTarget.value)}
            />
          </label>

          <label className="cluster-create-notify-field">
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
        </div>

        <div className="cluster-create-fields cluster-create-fields--pair">
          <label className="cluster-create-template-field">
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
        </div>

        <div className="cluster-create-actions">
          <button className="button" type="submit" disabled={!canSubmit}>
            <Plus aria-hidden="true" size={16} />
            {pending ? "创建中…" : "创建任务"}
          </button>
        </div>
      </form>
    </SectionCard>
  );
}
