import { X } from "lucide-react";
import { useEffect, useRef, useState, type MouseEvent } from "react";

import type { OrcTaskDto } from "../../bridge/types";
import { InlineError } from "../../components/InlineError";
import { toUserError } from "../../data/errors";
import { ORC_NOTIFY_MODE_OPTIONS, TASK_NAME_MAX_CHARS } from "./labels";

/** 编辑任务提交内容（goal = null 表示不改描述：已开始任务不允许改描述）。 */
export interface EditTaskInput {
  name: string;
  goal: string | null;
  notifyMode: string;
}

export interface EditTaskDialogProps {
  task: OrcTaskDto;
  pending: boolean;
  /** 保存失败原因（已转成用户可读文案）；对话框内展示，不关闭。 */
  error: unknown;
  onSubmit: (input: EditTaskInput) => Promise<boolean>;
  onClose: () => void;
}

/**
 * 编辑任务（集群页「编辑」）：名称随时可改；描述仅未开始任务可改；通知节奏随时可改。
 */
export function EditTaskDialog({
  task,
  pending,
  error,
  onSubmit,
  onClose,
}: EditTaskDialogProps) {
  const [name, setName] = useState(task.name);
  const [goal, setGoal] = useState(task.goal);
  const [notifyMode, setNotifyMode] = useState(task.notifyMode);
  const closeButtonRef = useRef<HTMLButtonElement>(null);
  const userError = error ? toUserError(error) : null;
  const started = task.started;

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

  const trimmedName = name.trim();
  const nameOk =
    trimmedName.length > 0 && [...trimmedName].length <= TASK_NAME_MAX_CHARS;
  const goalOk = started || goal.trim().length > 0;
  const canSubmit = nameOk && goalOk && !pending;

  const submit = async () => {
    if (!canSubmit) {
      return;
    }
    await onSubmit({
      name: trimmedName,
      goal: started ? null : goal.trim(),
      notifyMode,
    });
  };

  return (
    <div
      className="dialog-overlay"
      role="presentation"
      onMouseDown={handleOverlayMouseDown}
    >
      <section
        className="cluster-create-dialog cluster-edit-dialog"
        role="dialog"
        aria-modal="true"
        aria-labelledby="cluster-edit-title"
        aria-busy={pending}
      >
        <header className="dialog-header">
          <div>
            <h2 id="cluster-edit-title">编辑任务</h2>
            <p className="dialog-subtitle">
              名称用于列表与真实会话标题；描述是派给 Agent 的真实需求。
            </p>
          </div>
          <button
            ref={closeButtonRef}
            className="icon-button"
            type="button"
            aria-label="关闭编辑任务窗口"
            onClick={onClose}
          >
            <X aria-hidden="true" size={18} />
          </button>
        </header>

        <div className="cluster-dialog-body">
          {userError ? (
            <InlineError title={userError.title} message={userError.message} />
          ) : null}

          <div className="cluster-create-fields cluster-create-fields--pair">
            <label>
              <span>任务名称</span>
              <input
                aria-label="任务名称"
                type="text"
                maxLength={TASK_NAME_MAX_CHARS}
                placeholder="8 个字以内"
                value={name}
                disabled={pending}
                onChange={(event) => setName(event.currentTarget.value)}
              />
            </label>

            <label>
              <span>通知节奏</span>
              <select
                aria-label="通知节奏"
                value={notifyMode}
                disabled={pending}
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

          <label className="cluster-create-goal-field">
            <span>任务描述</span>
            <textarea
              aria-label="任务描述"
              className="cluster-create-goal"
              rows={3}
              value={goal}
              disabled={started || pending}
              onChange={(event) => setGoal(event.currentTarget.value)}
            />
          </label>

          {started ? (
            <p className="cluster-edit-note">
              任务已开始执行，描述不可再改（只可修改名称与通知节奏）。
            </p>
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
            className="button"
            type="button"
            disabled={!canSubmit}
            onClick={() => void submit()}
          >
            {pending ? "保存中…" : "保存修改"}
          </button>
        </div>
      </section>
    </div>
  );
}
