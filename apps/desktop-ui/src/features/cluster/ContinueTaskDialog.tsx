import { X } from "lucide-react";
import { useEffect, useRef, useState, type MouseEvent } from "react";

import type { OrcTaskDto } from "../../bridge/types";
import { InlineError } from "../../components/InlineError";
import { toUserError } from "../../data/errors";

/** 继续迭代提交内容：本轮要求可留空（交给项目经理按上一轮结论继续）。 */
export interface ContinueTaskInput {
  instruction: string | null;
}

export interface ContinueTaskDialogProps {
  task: OrcTaskDto;
  pending: boolean;
  /** 失败原因（已转成用户可读文案）；对话框内展示，不关闭。 */
  error: unknown;
  onSubmit: (input: ContinueTaskInput) => Promise<boolean>;
  onClose: () => void;
}

/**
 * 继续迭代（集群页）：本轮结束后开始新一轮——回到第 1 步由项目经理重新规划，
 * 可填写「本轮要求」（如复核意见/新需求）；留空则由项目经理按上一轮结论继续。
 */
export function ContinueTaskDialog({
  task,
  pending,
  error,
  onSubmit,
  onClose,
}: ContinueTaskDialogProps) {
  const [instruction, setInstruction] = useState("");
  const closeButtonRef = useRef<HTMLButtonElement>(null);
  const userError = error ? toUserError(error) : null;
  const nextRound = task.round + 1;

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

  const submit = async () => {
    if (pending) {
      return;
    }
    const trimmed = instruction.trim();
    await onSubmit({ instruction: trimmed.length > 0 ? trimmed : null });
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
        aria-labelledby="cluster-continue-title"
        aria-busy={pending}
      >
        <header className="dialog-header">
          <div>
            <h2 id="cluster-continue-title">
              继续迭代（第 {nextRound} 轮）
            </h2>
            <p className="dialog-subtitle">
              新一轮会回到第 1 步由项目经理重新规划，再走一遍实施与复核；
              项目经理会根据本轮结果决定是否还需要下一轮。
            </p>
          </div>
          <button
            ref={closeButtonRef}
            className="icon-button"
            type="button"
            aria-label="关闭继续迭代窗口"
            onClick={onClose}
          >
            <X aria-hidden="true" size={18} />
          </button>
        </header>

        <div className="cluster-dialog-body">
          {userError ? (
            <InlineError title={userError.title} message={userError.message} />
          ) : null}

          <label className="cluster-create-goal-field">
            <span>本轮要求（可留空）</span>
            <textarea
              aria-label="本轮要求"
              className="cluster-create-goal"
              rows={3}
              placeholder="例如：翅膀握住车把，腿自然弯曲；或直接贴复核意见"
              value={instruction}
              disabled={pending}
              onChange={(event) => setInstruction(event.currentTarget.value)}
            />
          </label>

          <p className="cluster-edit-note">
            留空 = 交给项目经理按上一轮结论继续（它会在汇总里列出下一轮要解决的问题）。
          </p>
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
            disabled={pending}
            onClick={() => void submit()}
          >
            {pending ? "启动中…" : "开始新一轮"}
          </button>
        </div>
      </section>
    </div>
  );
}
