import { Pause, Play } from "lucide-react";
import { useCallback, useEffect, useRef, useState } from "react";

import type { HostBridge } from "../bridge";
import type {
  RuntimeSnapshotDto,
  RuntimeSummaryDto,
} from "../bridge/types";
import { InlineError } from "./InlineError";

const RUNTIME_STATE_LABELS: Record<RuntimeSummaryDto["state"], string> = {
  Starting: "启动中",
  Running: "运行中",
  Paused: "已暂停",
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

function errorMessage(error: unknown): string {
  if (
    typeof error === "object" &&
    error !== null &&
    "message" in error &&
    typeof error.message === "string" &&
    error.message.trim()
  ) {
    return error.message;
  }
  return "运行时状态读取失败";
}

export function RuntimeStatusBar({ bridge }: RuntimeStatusBarProps) {
  const [snapshot, setSnapshot] = useState<RuntimeSnapshotDto | null>(null);
  const [isLoading, setIsLoading] = useState(true);
  const [isUpdating, setIsUpdating] = useState(false);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [actionError, setActionError] = useState<string | null>(null);
  const mountedRef = useRef(true);

  const loadSnapshot = useCallback(async () => {
    setIsLoading(true);
    setLoadError(null);
    try {
      const nextSnapshot = await bridge.invoke("get_snapshot", {});
      if (mountedRef.current) {
        setSnapshot(nextSnapshot);
      }
    } catch (error) {
      if (mountedRef.current) {
        setLoadError(errorMessage(error));
      }
    } finally {
      if (mountedRef.current) {
        setIsLoading(false);
      }
    }
  }, [bridge]);

  useEffect(() => {
    mountedRef.current = true;
    void loadSnapshot();
    return () => {
      mountedRef.current = false;
    };
  }, [loadSnapshot]);

  const togglePaused = async () => {
    const runtime = snapshot?.runtime;
    if (!runtime) {
      return;
    }

    const paused = !runtime.paused;
    setIsUpdating(true);
    setActionError(null);
    try {
      const nextRuntime = await bridge.invoke("set_runtime_paused", { paused });
      if (mountedRef.current) {
        setSnapshot((current) =>
          current ? { ...current, runtime: nextRuntime } : current,
        );
      }
    } catch (error) {
      if (mountedRef.current) {
        setActionError(errorMessage(error));
      }
    } finally {
      if (mountedRef.current) {
        setIsUpdating(false);
      }
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
    <section className="runtime-status-bar" aria-label="运行时状态">
      <div className="runtime-status-summary">
        <span
          className={`runtime-status-dot runtime-status-dot--${stateTone}`}
          aria-hidden="true"
        />
        <span className="runtime-status-label">{stateLabel}</span>
        {runtime ? (
          <span className="runtime-status-version">版本 {runtime.appVersion}</span>
        ) : null}
      </div>

      <div className="runtime-status-action">
        <button
          className="button button-secondary"
          type="button"
          disabled={!canToggle || isLoading || isUpdating}
          onClick={() => void togglePaused()}
        >
          <PauseIcon aria-hidden="true" size={16} />
          {isUpdating ? "正在更新" : pauseLabel}
        </button>
      </div>

      {loadError ? (
        <div className="runtime-status-error">
          <InlineError
            title="无法读取运行状态"
            message={`${loadError}。请重新检查运行时状态。`}
            action={
              <button
                className="button button-secondary"
                type="button"
                onClick={() => void loadSnapshot()}
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
            title="暂停操作未完成"
            message={`${actionError}。请确认运行时状态后重试。`}
          />
        </div>
      ) : null}
    </section>
  );
}
