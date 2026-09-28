import { ChevronDown } from "lucide-react";
import { Fragment, type ReactNode } from "react";

import type { OrcTaskDto } from "../../bridge/types";
import { StatusBadge } from "../../components/patterns";
import {
  ORC_NOTIFY_MODE_LABELS,
  orcTaskProgressLabel,
  orcTaskStateLabel,
  orcTaskStateTone,
} from "./labels";

export interface TaskListProps {
  tasks: OrcTaskDto[];
  /** 展开中的任务（手风琴：同一时间最多展开一个；进入页面不默认展开任何任务）。 */
  expandedTaskId: string | null;
  onToggle: (taskId: string) => void;
  /** 展开内容：任务详情（工作流 + 任务信息 + 操作）。 */
  renderDetail: (task: OrcTaskDto) => ReactNode;
}

/**
 * 任务列表（对齐渠道页）：整行可点手风琴，默认全部收起；
 * 行内展示目标、状态、进度、通知节奏；阻塞任务标红并显示原因。
 */
export function TaskList({
  tasks,
  expandedTaskId,
  onToggle,
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

            return (
              <Fragment key={task.id}>
                <tr
                  className={rowClasses.join(" ")}
                  onClick={() => onToggle(task.id)}
                >
                  <th scope="row">
                    <button
                      className="cluster-task-goal"
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
                      {task.goal}
                    </button>
                    {blocked && task.blockReason ? (
                      <span className="cluster-task-reason">
                        {task.blockReason}
                      </span>
                    ) : null}
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
                    {orcTaskProgressLabel(task)}
                  </td>
                  <td className="cluster-task-notify">
                    {ORC_NOTIFY_MODE_LABELS[task.notifyMode] ?? task.notifyMode}
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
                    <td colSpan={5}>
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
