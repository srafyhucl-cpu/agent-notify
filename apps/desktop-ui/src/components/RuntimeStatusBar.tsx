import { Minus, Pause, Play, Square, X } from "lucide-react";
import { useState } from "react";

import type { HostBridge } from "../bridge";
import type { RuntimeSummaryDto } from "../bridge/types";
import { toUserError } from "../data/errors";
import { useSetRuntimePausedMutation } from "../data/mutations";
import { useSnapshot } from "../data/useSnapshot";
import { InlineError } from "./InlineError";

export const RUNTIME_STATE_LABELS: Record<RuntimeSummaryDto["state"], string> = {
  Starting: "启动中",
  Running: "运行中",
  Paused: "已暂停",
  MigrationRequired: "迁移待处理",
  Stopping: "正在停止",
  Stopped: "已停止",
  Failed: "异常",
};

const RUNTIME_STATE_TONES: Record<
  RuntimeSummaryDto["state"],
  "success" | "waiting" | "danger" | "paused"
> = {
  Starting: "waiting",
  Running: "success",
  Paused: "paused",
  MigrationRequired: "danger",
  Stopping: "waiting",
  Stopped: "danger",
  Failed: "danger",
};

const TOGGLEABLE_RUNTIME_STATES = new Set<RuntimeSummaryDto["state"]>([
  "Starting",
  "Running",
  "Paused",
]);

export interface RuntimeStatusBarProps {
  bridge: HostBridge;
}

export function RuntimeStatusBar({ bridge }: RuntimeStatusBarProps) {
  const snapshotQuery = useSnapshot(bridge);
  const pauseMutation = useSetRuntimePausedMutation(bridge);
  const [pauseError, setPauseError] = useState<unknown>(null);
  const snapshot = snapshotQuery.data;
  const isLoading = snapshotQuery.isPending && !snapshot;
  const isUpdating = pauseMutation.isPending;
  const loadError = snapshotQuery.error
    ? toUserError(snapshotQuery.error)
    : null;
  const actionError = pauseError ? toUserError(pauseError) : null;

  const togglePaused = async () => {
    const runtime = snapshot?.runtime;
    if (!runtime) {
      return;
    }

    setPauseError(null);
    try {
      await pauseMutation.mutateAsync({ paused: !runtime.paused });
    } catch (error) {
      setPauseError(error);
    }
  };

  const runtime = snapshot?.runtime;
  const stateLabel = runtime
    ? RUNTIME_STATE_LABELS[runtime.state]
    : isLoading
      ? "正在读取"
      : "状态不可用";
  const stateTone = runtime
    ? RUNTIME_STATE_TONES[runtime.state]
    : isLoading
      ? "waiting"
      : "danger";
  const canToggle =
    runtime !== undefined && TOGGLEABLE_RUNTIME_STATES.has(runtime.state);
  const pauseLabel = runtime?.paused ? "恢复通知" : "暂停通知";
  const PauseIcon = runtime?.paused ? Play : Pause;

  return (
    <section className="runtime-status-bar" aria-label="运行时状态" data-tauri-drag-region>
      <div className="runtime-status-summary" data-tauri-drag-region>
        <span
          className={`runtime-status-dot runtime-status-dot--${stateTone}`}
          aria-hidden="true"
        />
        <span className="runtime-status-label">{stateLabel}</span>
        {runtime ? (
          <span className="runtime-status-version">版本 {runtime.appVersion}</span>
        ) : null}
      </div>

      <div className="runtime-status-right">
        <div className="runtime-status-action">
          <button
            className="button button-secondary"
            type="button"
            disabled={!canToggle || isLoading || isUpdating}
            onClick={() => void togglePaused()}
          >
            <PauseIcon aria-hidden="true" size={15} />
            {isUpdating ? "正在更新" : pauseLabel}
          </button>
        </div>

        <div className="window-controls" aria-label="窗口控制">
          <button
            type="button"
            className="window-control-btn"
            title="最小化"
            aria-label="最小化窗口"
            onClick={() => {
              void (async () => {
                try {
                  const { getCurrentWindow } = await import("@tauri-apps/api/window");
                  await getCurrentWindow().minimize();
                } catch {}
              })();
            }}
          >
            <Minus size={14} />
          </button>
          <button
            type="button"
            className="window-control-btn"
            title="最大化 / 还原"
            aria-label="最大化或还原窗口"
            onClick={() => {
              void (async () => {
                try {
                  const { getCurrentWindow } = await import("@tauri-apps/api/window");
                  await getCurrentWindow().toggleMaximize();
                } catch {}
              })();
            }}
          >
            <Square size={12} />
          </button>
          <button
            type="button"
            className="window-control-btn window-control-btn--close"
            title="关闭"
            aria-label="关闭窗口"
            onClick={() => {
              void (async () => {
                try {
                  const { getCurrentWindow } = await import("@tauri-apps/api/window");
                  await getCurrentWindow().close();
                } catch {}
              })();
            }}
          >
            <X size={15} />
          </button>
        </div>
      </div>

      {loadError ? (
        <div className="runtime-status-error">
          <InlineError
            title="无法读取运行状态"
            message={`${loadError.message}。请重新检查运行时状态。`}
            action={
              <button
                className="button button-secondary"
                type="button"
                onClick={() => void snapshotQuery.refetch()}
              >
                重新检查
              </button>
            }
          />
        </div>
      ) : null}

      {actionError ? (
        <div className="runtime-status-error">
          <InlineError
            title={actionError.title}
            message={`${actionError.message} 请确认运行时状态后重试。`}
          />
        </div>
      ) : null}
    </section>
  );
}
