import { Plus } from "lucide-react";
import { useState } from "react";

import type { HostBridge } from "../../bridge";
import type { OrcMessageKindDto } from "../../bridge/types";
import { EmptyState } from "../../components/EmptyState";
import { InlineError } from "../../components/InlineError";
import { LoadingRows } from "../../components/LoadingRows";
import { SectionCard } from "../../components/patterns";
import { toUserError } from "../../data/errors";
import {
  useAdvanceOrcTaskMutation,
  useCreateOrcTaskMutation,
  useRecoverBlockedOrcTaskMutation,
  useStartOrcTaskMutation,
} from "../../data/mutations";
import { useOpencodeProjects } from "../../data/useOpencodeProjects";
import { useOrcTasks } from "../../data/useOrcTasks";
import { useOrcTemplates } from "../../data/useOrcTemplates";
import {
  CreateTaskDialog,
  type CreateOrcTaskInput,
} from "./CreateTaskDialog";
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

  // 手风琴：同一时间最多展开一个任务；进入页面不默认展开任何任务。
  const [expandedTaskId, setExpandedTaskId] = useState<string | null>(null);
  const [createOpen, setCreateOpen] = useState(false);
  const [createError, setCreateError] = useState<unknown>(null);
  const [actionError, setActionError] = useState<unknown>(null);
  const [pendingAction, setPendingAction] = useState<
    "create" | "advance" | "recover" | "start" | null
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
              renderDetail={(task) => (
                <TaskDetail
                  task={task}
                  busy={rowBusy}
                  onStart={() => void handleStart(task.id)}
                  onAdvance={(kind) => void handleAdvance(task.id, kind)}
                  onRecover={() => void handleRecover(task.id)}
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
    </section>
  );
}
