import type { OrcTaskDto } from "../../bridge/types";
import { StatusBadge } from "../../components/patterns";
import {
  ORC_NOTIFY_MODE_LABELS,
  ORC_TASK_STATE_LABELS,
  ORC_TASK_STATE_TONES,
} from "./labels";

export interface TaskListProps {
  tasks: OrcTaskDto[];
  selectedTaskId: string | null;
  onSelect: (taskId: string) => void;
}

/** 任务列表：goal + 状态 + 当前步 + 通知节奏；阻塞任务标红提示原因。 */
export function TaskList({ tasks, selectedTaskId, onSelect }: TaskListProps) {
  return (
    <div className="cluster-task-list">
      <ul className="cluster-task-scroll" aria-label="任务列表">
        {tasks.map((task) => {
          const selected = task.id === selectedTaskId;
          const blocked = task.blockedStep !== null;
          const classes = ["cluster-task-row"];
          if (selected) {
            classes.push("cluster-task-row--selected");
          }
          if (blocked) {
            classes.push("cluster-task-row--blocked");
          }

          return (
            <li className="cluster-task-item" key={task.id}>
              <button
                type="button"
                className={classes.join(" ")}
                aria-pressed={selected}
                onClick={() => onSelect(task.id)}
              >
                <span className="cluster-task-goal">{task.goal}</span>
                <span className="cluster-task-meta">
                  <StatusBadge tone={ORC_TASK_STATE_TONES[task.state]}>
                    {ORC_TASK_STATE_LABELS[task.state]}
                  </StatusBadge>
                  {task.finalizing ? (
                    <StatusBadge tone="info">汇总中</StatusBadge>
                  ) : null}
                  {blocked ? (
                    <StatusBadge tone="danger">阻塞</StatusBadge>
                  ) : null}
                  <span className="cluster-task-step">
                    第 {task.currentStep} 步
                  </span>
                  <span className="cluster-task-notify">
                    {ORC_NOTIFY_MODE_LABELS[task.notifyMode] ?? task.notifyMode}
                  </span>
                </span>
                {blocked && task.blockReason ? (
                  <span className="cluster-task-reason">{task.blockReason}</span>
                ) : null}
              </button>
            </li>
          );
        })}
      </ul>
    </div>
  );
}