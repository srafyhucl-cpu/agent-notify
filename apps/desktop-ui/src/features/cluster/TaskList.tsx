import { ChevronDown, Pencil, Trash2 } from "lucide-react";
import { Fragment, type ReactNode } from "react";

import type { OrcTaskDto } from "../../bridge/types";
import { StatusBadge } from "../../components/patterns";
import {
  ORC_NOTIFY_MODE_LABELS,
  orcNodeStatesOfTask,
  orcTaskStateLabel,
  orcTaskStateTone,
  type OrcNodeState,
} from "./labels";

export interface TaskListProps {
  tasks: OrcTaskDto[];
  /** 展开中的任务（手风琴：同一时间最多展开一个；进入页面不默认展开任何任务）。 */
  expandedTaskId: string | null;
  onToggle: (taskId: string) => void;
  onEdit: (task: OrcTaskDto) => void;
  onDelete: (task: OrcTaskDto) => void;
  /** 展开内容：任务详情（工作流 + 任务信息 + 操作）。 */
  renderDetail: (task: OrcTaskDto) => ReactNode;
}

/** 节点状态 → 进度段样式（预览态 idle 不出现，兜底 pending）。 */
function stepDotClass(state: OrcNodeState | undefined): string {
  switch (state) {
    case "done":
      return "cluster-step-dot cluster-step-dot--done";
    case "current":
    case "finalizing":
      return "cluster-step-dot cluster-step-dot--current";
    case "failed":
      return "cluster-step-dot cluster-step-dot--failed";
    default:
      return "cluster-step-dot";
  }
}

/**
 * 任务列表（对齐渠道页）：整行可点手风琴，默认全部收起；
 * 行内只展示任务名称（短名）、状态、分步进度段与通知节奏；
 * 行尾提供编辑/删除（增删改查）；失败原因在展开详情里展示（列表不堆报错）。
 */
export function TaskList({
  tasks,
  expandedTaskId,
  onToggle,
  onEdit,
  onDelete,
  renderDetail,
}: TaskListProps) {
  return (
    <div className="cluster-task-table">
      <table>
        <caption className="visually-hidden">编排任务列表</caption>
        <thead>
          <tr>
            <th scope="col">任务</th>
            <th scope="col" className="cluster-th-state">
              状态
            </th>
            <th scope="col" className="cluster-th-progress">
              进度
            </th>
            <th scope="col" className="cluster-th-notify">
              通知节奏
            </th>
            <th scope="col" className="cluster-th-actions">
              <span className="visually-hidden">操作</span>
            </th>
            <th scope="col" className="cluster-th-expand">
              <span className="visually-hidden">展开</span>
            </th>
          </tr>
        </thead>
        <tbody>
          {tasks.map((task) => {
            const expanded = task.id === expandedTaskId;
            const blocked = task.blockedStep !== null;
            const rowClasses = ["cluster-task-row"];
            if (expanded) {
              rowClasses.push("cluster-task-row--expanded");
            }
            if (blocked) {
              rowClasses.push("cluster-task-row--blocked");
            }
            const nodeStates = orcNodeStatesOfTask(task);

            return (
              <Fragment key={task.id}>
                <tr
                  className={rowClasses.join(" ")}
                  onClick={() => onToggle(task.id)}
                >
                  <th scope="row">
                    <button
                      className="cluster-task-name"
                      type="button"
                      aria-expanded={expanded}
                      aria-controls={
                        expanded ? `cluster-task-detail-${task.id}` : undefined
                      }
                      onClick={(event) => {
                        event.stopPropagation();
                        onToggle(task.id);
                      }}
                    >
                      {task.name}
                    </button>
                  </th>
                  <td>
                    <span className="cluster-task-badges">
                      <StatusBadge tone={orcTaskStateTone(task)}>
                        {orcTaskStateLabel(task)}
                      </StatusBadge>
                      {task.finalizing ? (
                        <StatusBadge tone="info">汇总中</StatusBadge>
                      ) : null}
                      {blocked ? (
                        <StatusBadge tone="danger">阻塞</StatusBadge>
                      ) : null}
                    </span>
                  </td>
                  <td className="cluster-task-progress">
                    <span className="cluster-step-track" aria-hidden="true">
                      {task.workflow.steps.map((step) => (
                        <span
                          className={stepDotClass(nodeStates[step.order])}
                          key={step.order}
                        />
                      ))}
                    </span>
                  </td>
                  <td className="cluster-task-notify">
                    {ORC_NOTIFY_MODE_LABELS[task.notifyMode] ?? task.notifyMode}
                  </td>
                  <td
                    className="cluster-task-actions-cell"
                    onClick={(event) => event.stopPropagation()}
                  >
                    <button
                      className="button-icon-subtle"
                      type="button"
                      title="编辑任务"
                      aria-label={`编辑任务 ${task.name}`}
                      onClick={() => onEdit(task)}
                    >
                      <Pencil size={13} aria-hidden="true" />
                    </button>
                    <button
                      className="button-icon-subtle button-icon-danger"
                      type="button"
                      title="删除任务"
                      aria-label={`删除任务 ${task.name}`}
                      onClick={() => onDelete(task)}
                    >
                      <Trash2 size={13} aria-hidden="true" />
                    </button>
                  </td>
                  <td className="cluster-task-expand-cell">
                    <ChevronDown
                      className={`cluster-chevron ${
                        expanded ? "cluster-chevron--open" : ""
                      }`}
                      aria-hidden="true"
                      size={16}
                    />
                  </td>
                </tr>

                {expanded ? (
                  <tr className="cluster-task-detail-row">
                    <td colSpan={6}>
                      <div
                        className="cluster-task-inline-detail"
                        id={`cluster-task-detail-${task.id}`}
                      >
                        {renderDetail(task)}
                      </div>
                    </td>
                  </tr>
                ) : null}
              </Fragment>
            );
          })}
        </tbody>
      </table>
    </div>
  );
}
