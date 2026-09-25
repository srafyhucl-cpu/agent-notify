import { Wrench } from "lucide-react";
import { useState } from "react";

import type { HostBridge } from "../../bridge";
import type {
  ComponentStateDto,
  DiagnosticActionDto,
  DiagnosticItemDto,
  DiagnosticLevelDto,
} from "../../bridge/types";
import { InlineError } from "../../components/InlineError";
import {
  StatusBadge,
  type StatusBadgeTone,
} from "../../components/patterns";
import { toUserError } from "../../data/errors";
import { useDiagnosticActionMutation } from "../../data/mutations";

const LEVEL_LABELS: Record<DiagnosticLevelDto, string> = {
  Normal: "正常",
  Waiting: "等待",
  Error: "异常",
  Paused: "暂停",
};

/** 严重度 → StatusBadge 语义色（颜色 + 文字双载，不靠颜色单载语义）。 */
const LEVEL_TONES: Record<DiagnosticLevelDto, StatusBadgeTone> = {
  Normal: "success",
  Waiting: "warning",
  Error: "danger",
  Paused: "info",
};

/** 严重度分组：错误 > 等待（含暂停）> 正常；仅渲染非空分组，组间以 24px 间距区隔。 */
const SEVERITY_GROUPS: ReadonlyArray<{
  key: string;
  title: string;
  levels: ReadonlyArray<DiagnosticLevelDto>;
}> = [
  { key: "error", title: "错误", levels: ["Error"] },
  { key: "waiting", title: "等待", levels: ["Waiting", "Paused"] },
  { key: "normal", title: "正常", levels: ["Normal"] },
];

export const COMPONENT_STATE_LABELS: Record<ComponentStateDto, string> = {
  Starting: "启动中",
  Running: "运行中",
  Paused: "已暂停",
  Stopped: "已停止",
  Failed: "异常",
};

function formatTimestamp(value: string): string {
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) {
    return value;
  }
  return date.toLocaleString("zh-CN", {
    year: "numeric",
    month: "2-digit",
    day: "2-digit",
    hour: "2-digit",
    minute: "2-digit",
    second: "2-digit",
  });
}

function DiagnosticItem({
  bridge,
  item,
}: {
  bridge: HostBridge;
  item: DiagnosticItemDto;
}) {
  const [actionError, setActionError] = useState<unknown>(null);
  const action = item.action;
  const actionMutation = useDiagnosticActionMutation(
    bridge,
    action?.command ?? "get_diagnostics",
  );
  const error = actionError ? toUserError(actionError) : null;

  const runAction = async (declaredAction: DiagnosticActionDto) => {
    setActionError(null);
    try {
      await actionMutation.mutateAsync(declaredAction.payload as never);
    } catch (mutationError) {
      setActionError(mutationError);
    }
  };

  return (
    <article className="diagnostic-item">
      <div className="diagnostic-item-main">
        <div className="diagnostic-item-heading">
          <StatusBadge tone={LEVEL_TONES[item.level]}>
            {LEVEL_LABELS[item.level]}
          </StatusBadge>
          <code>{item.code}</code>
        </div>
        <p className="diagnostic-message">{item.message}</p>
        <p className="diagnostic-time">
          最近检查：{formatTimestamp(item.checkedAt)}
        </p>
        {error ? (
          <InlineError title={error.title} message={error.message} />
        ) : null}
      </div>

      <div className="diagnostic-action">
        {action ? (
          <button
            className="button button-secondary"
            type="button"
            disabled={actionMutation.isPending}
            onClick={() => void runAction(action)}
          >
            <Wrench aria-hidden="true" size={15} />
            {actionMutation.isPending ? "正在执行" : action.label}
          </button>
        ) : (
          <span className="diagnostic-action-unavailable">
            当前没有可执行的修复动作
          </span>
        )}
      </div>
    </article>
  );
}

export interface DiagnosticListProps {
  bridge: HostBridge;
  items: DiagnosticItemDto[];
}

export function DiagnosticList({ bridge, items }: DiagnosticListProps) {
  if (items.length === 0) {
    return (
      <p className="section-empty">StatusService 当前没有返回诊断项；运行与存储检查未发现问题，因此没有需要处理的项。</p>
    );
  }

  const groups = SEVERITY_GROUPS.map((group) => ({
    ...group,
    items: items.filter((item) => group.levels.includes(item.level)),
  })).filter((group) => group.items.length > 0);

  return (
    <div className="diagnostic-list" aria-label="StatusService 诊断项">
      {groups.map((group) => (
        <div className="diagnostic-group" key={group.key}>
          <h3 className="diagnostic-group-title">
            {group.title} ·{" "}
            <span className="diagnostic-group-count">{group.items.length}</span>
          </h3>
          {group.items.map((item) => (
            <DiagnosticItem bridge={bridge} item={item} key={item.code} />
          ))}
        </div>
      ))}
    </div>
  );
}
