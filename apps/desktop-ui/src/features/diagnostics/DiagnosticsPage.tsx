import { Clipboard, RefreshCw } from "lucide-react";
import { useState } from "react";

import type { HostBridge } from "../../bridge";
import type { DiagnosticsDto } from "../../bridge/types";
import { EmptyState } from "../../components/EmptyState";
import { InlineError } from "../../components/InlineError";
import { LoadingRows } from "../../components/LoadingRows";
import { toUserError } from "../../data/errors";
import { useDiagnostics } from "../../data/useDiagnostics";
import {
  COMPONENT_STATE_LABELS,
  DiagnosticList,
} from "./DiagnosticList";

function safeDiagnosticsText(diagnostics: DiagnosticsDto): string {
  return JSON.stringify(
    {
      generatedAt: diagnostics.generatedAt,
      runtime: diagnostics.runtime,
      storage: diagnostics.storage,
      components: diagnostics.components,
      items: diagnostics.items,
    },
    null,
    2,
  );
}

export interface DiagnosticsPageProps {
  bridge: HostBridge;
}

export function DiagnosticsPage({ bridge }: DiagnosticsPageProps) {
  const diagnosticsQuery = useDiagnostics(bridge);
  const [copyState, setCopyState] = useState<
    | { kind: "success"; message: string }
    | { kind: "error"; message: string }
    | null
  >(null);
  const diagnostics = diagnosticsQuery.data;
  const loadError = diagnosticsQuery.error
    ? toUserError(diagnosticsQuery.error)
    : null;

  const copyDiagnostics = async () => {
    if (!diagnostics) {
      return;
    }
    if (!navigator.clipboard) {
      setCopyState({
        kind: "error",
        message: "当前环境不支持复制，请在 Diagnostics 页面查看安全诊断信息。",
      });
      return;
    }

    try {
      await navigator.clipboard.writeText(safeDiagnosticsText(diagnostics));
      setCopyState({ kind: "success", message: "安全诊断信息已复制" });
    } catch {
      setCopyState({
        kind: "error",
        message: "复制未完成，请稍后重试。",
      });
    }
  };

  return (
    <section className="workbench-page" aria-labelledby="page-title-diagnostics">
      <header className="workbench-page-header">
        <div>
          <h1 className="workbench-page-title" id="page-title-diagnostics">
            Diagnostics
          </h1>
          <p className="page-summary">
            所有诊断结论直接来自 StatusService，页面只展示和刷新这些结果。
          </p>
        </div>
        <div className="page-actions">
          <button
            className="button button-secondary"
            type="button"
            disabled={diagnosticsQuery.isFetching}
            onClick={() => void diagnosticsQuery.refetch()}
          >
            <RefreshCw aria-hidden="true" size={15} />
            {diagnosticsQuery.isFetching ? "正在刷新" : "刷新"}
          </button>
          <button
            className="button"
            type="button"
            disabled={!diagnostics}
            onClick={() => void copyDiagnostics()}
          >
            <Clipboard aria-hidden="true" size={15} />
            复制安全诊断信息
          </button>
        </div>
      </header>

      <div className="workbench-page-content diagnostics-page-content">
        {loadError ? (
          <InlineError
            title="无法读取诊断状态"
            message={loadError.message}
            action={
              <button
                className="button button-secondary"
                type="button"
                onClick={() => void diagnosticsQuery.refetch()}
              >
                重新检查
              </button>
            }
          />
        ) : null}

        {copyState ? (
          <p
            className={
              copyState.kind === "success"
                ? "settings-feedback settings-feedback--success"
                : "settings-feedback settings-feedback--error"
            }
            role={copyState.kind === "error" ? "alert" : "status"}
          >
            {copyState.message}
          </p>
        ) : null}

        {diagnosticsQuery.isPending && !diagnostics ? (
          <LoadingRows aria-label="正在读取诊断状态" rows={6} />
        ) : null}

        {!diagnosticsQuery.isPending && !diagnostics && !loadError ? (
          <EmptyState
            title="暂无诊断状态"
            description="StatusService 尚未返回诊断信息。"
          />
        ) : null}

        {diagnostics ? (
          <>
            <section
              className="diagnostics-summary"
              aria-labelledby="diagnostics-summary-title"
            >
              <header className="section-heading">
                <div>
                  <h2 id="diagnostics-summary-title">状态摘要</h2>
                  <p className="section-description">
                    最近生成：{diagnostics.generatedAt}
                  </p>
                </div>
              </header>
              <dl className="diagnostics-summary-list">
                <div>
                  <dt>应用版本</dt>
                  <dd>{diagnostics.runtime.appVersion}</dd>
                </div>
                <div>
                  <dt>运行平台</dt>
                  <dd>{diagnostics.runtime.platform}</dd>
                </div>
                <div>
                  <dt>运行状态</dt>
                  <dd>{diagnostics.runtime.state}</dd>
                </div>
                <div>
                  <dt>通知 / 投递</dt>
                  <dd>
                    {diagnostics.storage.notificationCount} /{" "}
                    {diagnostics.storage.deliveryCount}
                  </dd>
                </div>
                <div>
                  <dt>待发送</dt>
                  <dd>{diagnostics.storage.pendingOutboxCount}</dd>
                </div>
                <div>
                  <dt>最近存储错误</dt>
                  <dd>{diagnostics.storage.recentError?.message ?? "无"}</dd>
                </div>
              </dl>
            </section>

            <section
              className="diagnostics-section"
              aria-labelledby="diagnostics-items-title"
            >
              <header className="section-heading">
                <div>
                  <h2 id="diagnostics-items-title">诊断项</h2>
                  <p className="section-description">
                    修复动作仅执行 StatusService 声明的 HostBridge 命令。
                  </p>
                </div>
              </header>
              <DiagnosticList bridge={bridge} items={diagnostics.items} />
            </section>

            <section
              className="diagnostics-section"
              aria-labelledby="diagnostics-components-title"
            >
              <header className="section-heading">
                <div>
                  <h2 id="diagnostics-components-title">组件状态</h2>
                </div>
              </header>
              {diagnostics.components.length === 0 ? (
                <p className="section-empty">StatusService 未返回组件状态。</p>
              ) : (
                <div className="diagnostic-components">
                  {diagnostics.components.map((component) => (
                    <div className="diagnostic-component" key={component.name}>
                      <strong>{component.name}</strong>
                      <span>{COMPONENT_STATE_LABELS[component.state]}</span>
                      <p>{component.detail?.message ?? "无可见错误"}</p>
                    </div>
                  ))}
                </div>
              )}
            </section>
          </>
        ) : null}
      </div>
    </section>
  );
}
