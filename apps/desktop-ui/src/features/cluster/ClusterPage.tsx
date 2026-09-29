import { AlertTriangle, Plus, X } from "lucide-react";
import { useEffect, useRef, useState } from "react";

import type { HostBridge } from "../../bridge";
import type { OrcMessageKindDto, OrcTaskDto } from "../../bridge/types";
import { EmptyState } from "../../components/EmptyState";
import { InlineError } from "../../components/InlineError";
import { LoadingRows } from "../../components/LoadingRows";
import { SectionCard } from "../../components/patterns";
import { toUserError } from "../../data/errors";
import {
  useAdvanceOrcTaskMutation,
  useContinueOrcTaskMutation,
  useCreateOrcTaskMutation,
  useDeleteOrcTaskMutation,
  useRecoverBlockedOrcTaskMutation,
  useStartOrcTaskMutation,
  useUpdateOrcTaskMutation,
} from "../../data/mutations";
import { useOpencodeProjects } from "../../data/useOpencodeProjects";
import { useOrcTasks } from "../../data/useOrcTasks";
import { useOrcTemplates } from "../../data/useOrcTemplates";
import {
  CreateTaskDialog,
  type CreateOrcTaskInput,
} from "./CreateTaskDialog";
import {
  ContinueTaskDialog,
  type ContinueTaskInput,
} from "./ContinueTaskDialog";
import { EditTaskDialog, type EditTaskInput } from "./EditTaskDialog";
import { TaskDetail } from "./TaskDetail";
import { TaskList } from "./TaskList";

export interface ClusterPageProps {
  bridge: HostBridge;
}

/**
 * 集群页（2026-09 动线重做）：对齐渠道页的「单卡 + 行内手风琴」结构——
 * 右上角「新建任务」弹窗里选模板并逐个节点配置 Agent/模型（创建即锁定）；
 * 任务列表默认全部收起，点行展开才看工作流与操作，不再默认展开某个任务详情。
 */
export function ClusterPage({ bridge }: ClusterPageProps) {
  const tasksQuery = useOrcTasks(bridge);
  const templatesQuery = useOrcTemplates(bridge);
  const projectsQuery = useOpencodeProjects(bridge);
  const createMutation = useCreateOrcTaskMutation(bridge);
  const advanceMutation = useAdvanceOrcTaskMutation(bridge);
  const recoverMutation = useRecoverBlockedOrcTaskMutation(bridge);
  const startMutation = useStartOrcTaskMutation(bridge);
  const updateMutation = useUpdateOrcTaskMutation(bridge);
  const deleteMutation = useDeleteOrcTaskMutation(bridge);
  const continueMutation = useContinueOrcTaskMutation(bridge);

  // 手风琴：同一时间最多展开一个任务；进入页面不默认展开任何任务。
  const [expandedTaskId, setExpandedTaskId] = useState<string | null>(null);
  const [createOpen, setCreateOpen] = useState(false);
  const [createError, setCreateError] = useState<unknown>(null);
  const [actionError, setActionError] = useState<unknown>(null);
  const [editTask, setEditTask] = useState<OrcTaskDto | null>(null);
  const [editError, setEditError] = useState<unknown>(null);
  const [deleteTarget, setDeleteTarget] = useState<OrcTaskDto | null>(null);
  const [deleteError, setDeleteError] = useState<unknown>(null);
  const [continueTask, setContinueTask] = useState<OrcTaskDto | null>(null);
  const [continueError, setContinueError] = useState<unknown>(null);
  const cancelDeleteRef = useRef<HTMLButtonElement>(null);
  const [pendingAction, setPendingAction] = useState<
    "create" | "advance" | "recover" | "start" | "update" | "delete" | "continue" | null
  >(null);

  const tasks = tasksQuery.data ?? [];
  const loadError = tasksQuery.error ? toUserError(tasksQuery.error) : null;
  const actionUserError = actionError ? toUserError(actionError) : null;
  const templatesError = templatesQuery.error
    ? toUserError(templatesQuery.error).message
    : null;
  const projectsError = projectsQuery.error
    ? toUserError(projectsQuery.error).message
    : null;
  // 行内操作（开始/推进/重新发起）的进行中态；创建由弹窗自己的按钮承担。
  const rowBusy = pendingAction !== null && pendingAction !== "create";

  /**
   * 创建任务（弹窗提交）：成功后展开新任务、关闭弹窗；
   * 「创建并开始」再紧接着派活第 1 步——派活失败只在页面顶部如实提示，任务保持待开始可重试。
   */
  const handleCreate = async (
    input: CreateOrcTaskInput,
    startImmediately: boolean,
  ): Promise<boolean> => {
    setCreateError(null);
    setActionError(null);
    setPendingAction("create");
    try {
      const created = await createMutation.mutateAsync(input);
      setExpandedTaskId(created.id);
      setCreateOpen(false);
      if (startImmediately) {
        try {
          await startMutation.mutateAsync({ taskId: created.id });
        } catch (error) {
          setActionError(error);
        }
      }
      return true;
    } catch (error) {
      setCreateError(error);
      return false;
    } finally {
      setPendingAction(null);
    }
  };

  const handleAdvance = async (taskId: string, kind: OrcMessageKindDto) => {
    setActionError(null);
    setPendingAction("advance");
    try {
      await advanceMutation.mutateAsync({ taskId, kind });
    } catch (error) {
      setActionError(error);
    } finally {
      setPendingAction(null);
    }
  };

  const handleRecover = async (taskId: string) => {
    setActionError(null);
    setPendingAction("recover");
    try {
      await recoverMutation.mutateAsync({ taskId });
    } catch (error) {
      setActionError(error);
    } finally {
      setPendingAction(null);
    }
  };

  const handleStart = async (taskId: string) => {
    setActionError(null);
    setPendingAction("start");
    try {
      await startMutation.mutateAsync({ taskId });
    } catch (error) {
      setActionError(error);
    } finally {
      setPendingAction(null);
    }
  };

  /** 保存编辑：成功关闭弹窗；失败在弹窗内展示并保留输入。 */
  const handleUpdate = async (input: EditTaskInput): Promise<boolean> => {
    if (!editTask) {
      return false;
    }
    setEditError(null);
    setPendingAction("update");
    try {
      await updateMutation.mutateAsync({
        taskId: editTask.id,
        name: input.name,
        goal: input.goal,
        notifyMode: input.notifyMode,
      });
      setEditTask(null);
      return true;
    } catch (error) {
      setEditError(error);
      return false;
    } finally {
      setPendingAction(null);
    }
  };

  /** 删除任务（界面已确认）：成功收起详情与确认框；失败展示明确原因。 */
  const handleDelete = async () => {
    if (!deleteTarget) {
      return;
    }
    setDeleteError(null);
    setPendingAction("delete");
    try {
      await deleteMutation.mutateAsync({ taskId: deleteTarget.id });
      if (expandedTaskId === deleteTarget.id) {
        setExpandedTaskId(null);
      }
      setDeleteTarget(null);
    } catch (error) {
      setDeleteError(error);
    } finally {
      setPendingAction(null);
    }
  };

  /** 继续迭代：开始新一轮（轮次 +1、回到第 1 步）；成功关闭弹窗，失败在弹窗内展示。 */
  const handleContinue = async (input: ContinueTaskInput): Promise<boolean> => {
    if (!continueTask) {
      return false;
    }
    setContinueError(null);
    setPendingAction("continue");
    try {
      await continueMutation.mutateAsync({
        taskId: continueTask.id,
        instruction: input.instruction,
      });
      setContinueTask(null);
      return true;
    } catch (error) {
      setContinueError(error);
      return false;
    } finally {
      setPendingAction(null);
    }
  };

  // 删除确认：Escape 关闭；打开时把焦点移到「取消」（破坏性操作不自动聚焦）。
  useEffect(() => {
    if (!deleteTarget) {
      return;
    }
    cancelDeleteRef.current?.focus();
    const handleKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        setDeleteTarget(null);
      }
    };
    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, [deleteTarget]);

  return (
    <section className="workbench-page cluster-page">
      <h1 className="visually-hidden">集群</h1>
      <div className="workbench-page-content cluster-page-content">
        {loadError ? (
          <InlineError
            title="无法读取集群任务"
            message={loadError.message}
            action={
              <button
                className="button button-secondary"
                type="button"
                onClick={() => void tasksQuery.refetch()}
              >
                重新检查
              </button>
            }
          />
        ) : null}

        {actionUserError ? (
          <InlineError
            title={actionUserError.title}
            message={actionUserError.message}
          />
        ) : null}

        <SectionCard
          title="编排任务"
          count={tasks.length}
          description="按固定工作流依次唤醒 Agent（首步是项目经理，最后汇总汇报）；点任务行展开查看工作流与进度。"
          action={
            <button
              className="button button-secondary"
              type="button"
              onClick={() => {
                setCreateError(null);
                setCreateOpen(true);
              }}
            >
              <Plus aria-hidden="true" size={15} />
              新建任务
            </button>
          }
        >
          {tasksQuery.isPending && !tasksQuery.data ? (
            <LoadingRows aria-label="正在加载集群任务" rows={4} />
          ) : null}

          {!tasksQuery.isPending && tasks.length === 0 && !loadError ? (
            <EmptyState
              title="暂无集群任务"
              description="点右上角「新建任务」按固定模板创建：为每个节点选好 Agent 后，编排层会逐步唤醒对应 Agent，进度显示在这里。"
            />
          ) : null}

          {tasks.length > 0 ? (
            <TaskList
              tasks={tasks}
              expandedTaskId={expandedTaskId}
              onToggle={(taskId) =>
                setExpandedTaskId((current) =>
                  current === taskId ? null : taskId,
                )
              }
              onEdit={(task) => {
                setEditError(null);
                setEditTask(task);
              }}
              onDelete={(task) => {
                setDeleteError(null);
                setDeleteTarget(task);
              }}
              renderDetail={(task) => (
                <TaskDetail
                  task={task}
                  busy={rowBusy}
                  onStart={() => void handleStart(task.id)}
                  onAdvance={(kind) => void handleAdvance(task.id, kind)}
                  onRecover={() => void handleRecover(task.id)}
                  onContinue={() => {
                    setContinueError(null);
                    setContinueTask(task);
                  }}
                />
              )}
            />
          ) : null}
        </SectionCard>
      </div>

      {createOpen ? (
        <CreateTaskDialog
          bridge={bridge}
          pending={pendingAction === "create"}
          templates={templatesQuery.data ?? null}
          templatesError={templatesError}
          onRetryTemplates={() => void templatesQuery.refetch()}
          projects={projectsQuery.data ?? null}
          projectsError={projectsError}
          onRetryProjects={() => void projectsQuery.refetch()}
          error={createError}
          onSubmit={handleCreate}
          onClose={() => setCreateOpen(false)}
        />
      ) : null}

      {editTask ? (
        <EditTaskDialog
          task={editTask}
          pending={pendingAction === "update"}
          error={editError}
          onSubmit={handleUpdate}
          onClose={() => setEditTask(null)}
        />
      ) : null}

      {continueTask ? (
        <ContinueTaskDialog
          task={continueTask}
          pending={pendingAction === "continue"}
          error={continueError}
          onSubmit={handleContinue}
          onClose={() => setContinueTask(null)}
        />
      ) : null}

      {deleteTarget ? (
        <div className="dialog-overlay" role="presentation">
          <section
            className="confirm-dialog"
            role="alertdialog"
            aria-modal="true"
            aria-labelledby="cluster-delete-title"
          >
            <AlertTriangle aria-hidden="true" size={24} />
            <div>
              <h2 id="cluster-delete-title">
                删除任务「{deleteTarget.name}」
              </h2>
              <p>
                删除后任务记录不再显示；已派活的 OpenCode
                会话不会停止，后续进度也不再跟踪。此操作不可恢复。
              </p>
              {deleteError ? (
                <InlineError
                  title="删除失败"
                  message={toUserError(deleteError).message}
                />
              ) : null}
            </div>
            <div className="confirm-dialog-actions">
              <button
                className="button button-secondary"
                type="button"
                ref={cancelDeleteRef}
                disabled={pendingAction === "delete"}
                onClick={() => setDeleteTarget(null)}
              >
                <X aria-hidden="true" size={15} />
                取消
              </button>
              <button
                className="button button-danger"
                type="button"
                aria-label="确认删除"
                disabled={pendingAction === "delete"}
                onClick={() => void handleDelete()}
              >
                {pendingAction === "delete" ? "删除中…" : "确认删除"}
              </button>
            </div>
          </section>
        </div>
      ) : null}
    </section>
  );
}
