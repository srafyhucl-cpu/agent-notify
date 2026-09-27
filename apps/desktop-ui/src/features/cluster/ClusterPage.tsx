import { Network } from "lucide-react";
import { useMemo, useState } from "react";

import type { HostBridge } from "../../bridge";
import type { OrcMessageKindDto } from "../../bridge/types";
import { EmptyState } from "../../components/EmptyState";
import { InlineError } from "../../components/InlineError";
import { LoadingRows } from "../../components/LoadingRows";
import { PageHeader } from "../../components/patterns";
import { toUserError } from "../../data/errors";
import {
  useAdvanceOrcTaskMutation,
  useCreateOrcTaskMutation,
  useRecoverBlockedOrcTaskMutation,
} from "../../data/mutations";
import { useOrcTasks } from "../../data/useOrcTasks";
import { CreateTaskForm } from "./CreateTaskForm";
import { TaskDetail } from "./TaskDetail";
import { TaskList } from "./TaskList";

export interface ClusterPageProps {
  bridge: HostBridge;
}

/**
 * 集群页（P1，§6.2 桌面集群形态）：任务列表 + 详情/发指令 + 创建入口。
 * 只消费现有编排 DTO/命令；推进命令经 HostBridge 调用 advance_orc_task / recover_blocked_orc_task。
 */
export function ClusterPage({ bridge }: ClusterPageProps) {
  const tasksQuery = useOrcTasks(bridge);
  const createMutation = useCreateOrcTaskMutation(bridge);
  const advanceMutation = useAdvanceOrcTaskMutation(bridge);
  const recoverMutation = useRecoverBlockedOrcTaskMutation(bridge);

  const [selectedTaskId, setSelectedTaskId] = useState<string | null>(null);
  const [createError, setCreateError] = useState<unknown>(null);
  const [actionError, setActionError] = useState<unknown>(null);
  const [pendingAction, setPendingAction] = useState<
    "advance" | "recover" | null
  >(null);

  const tasks = tasksQuery.data ?? [];
  // 默认选中列表首项；选中项被刷新移除时回落到首项，避免详情指向不存在的任务。
  const selectedTask = useMemo(
    () =>
      tasks.find((task) => task.id === selectedTaskId) ?? tasks[0] ?? null,
    [selectedTaskId, tasks],
  );
  const loadError = tasksQuery.error ? toUserError(tasksQuery.error) : null;
  const createUserError = createError ? toUserError(createError) : null;
  const actionUserError = actionError ? toUserError(actionError) : null;

  const handleCreate = async (goal: string, notifyMode: string): Promise<boolean> => {
    setCreateError(null);
    try {
      const created = await createMutation.mutateAsync({ goal, notifyMode });
      setSelectedTaskId(created.id);
      return true;
    } catch (error) {
      setCreateError(error);
      return false;
    }
  };

  const handleAdvance = async (
    taskId: string,
    kind: OrcMessageKindDto,
  ) => {
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

  return (
    <section className="workbench-page cluster-page" aria-label="集群">
      <PageHeader
        title="集群"
        summary="查看编排任务进度，向集群下达指令；阻塞任务可在此重新发起。"
        actions={
          <span className="page-count" aria-label={`共 ${tasks.length} 个任务`}>
            <Network aria-hidden="true" size={16} />
            {tasks.length} 个
          </span>
        }
      />

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

        {createUserError ? (
          <InlineError
            title={createUserError.title}
            message={createUserError.message}
          />
        ) : null}

        {actionUserError ? (
          <InlineError title={actionUserError.title} message={actionUserError.message} />
        ) : null}

        <CreateTaskForm
          pending={createMutation.isPending}
          onSubmit={handleCreate}
        />

        {tasksQuery.isPending && !tasksQuery.data ? (
          <LoadingRows aria-label="正在加载集群任务" rows={4} />
        ) : null}

        {!tasksQuery.isPending && tasks.length === 0 && !loadError ? (
          <EmptyState
            title="暂无集群任务"
            description="创建第一个任务后，编排层会按预置工作流逐步唤醒对应 Agent，进度会显示在这里。"
          />
        ) : null}

        {tasks.length > 0 ? (
          <div className="cluster-workspace">
            <TaskList
              tasks={tasks}
              selectedTaskId={selectedTask?.id ?? null}
              onSelect={setSelectedTaskId}
            />
            <TaskDetail
              task={selectedTask}
              busy={pendingAction !== null}
              onAdvance={(kind) => {
                if (selectedTask) {
                  void handleAdvance(selectedTask.id, kind);
                }
              }}
              onRecover={() => {
                if (selectedTask) {
                  void handleRecover(selectedTask.id);
                }
              }}
            />
          </div>
        ) : null}
      </div>
    </section>
  );
}