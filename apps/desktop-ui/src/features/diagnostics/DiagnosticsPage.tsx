import { useQueryClient } from "@tanstack/react-query";
import { Clipboard, FileText, RefreshCw } from "lucide-react";
import { useState } from "react";

import type { HostBridge } from "../../bridge";
import type {
  DiagnosticsDto,
  LegacyMigrationDto,
  MigrationIssueDto,
  MigrationReportDto,
  MigrationStateDto,
} from "../../bridge/types";
import { EmptyState } from "../../components/EmptyState";
import { InlineError } from "../../components/InlineError";
import { LoadingRows } from "../../components/LoadingRows";
import { toUserError } from "../../data/errors";
import { useDiagnosticActionMutation } from "../../data/mutations";
import { queryKeys } from "../../data/queryKeys";
import { useDiagnostics } from "../../data/useDiagnostics";
import {
  COMPONENT_STATE_LABELS,
  DiagnosticList,
} from "./DiagnosticList";

const MIGRATION_STATE_LABELS: Record<MigrationStateDto, string> = {
  NotConfigured: "未发现旧数据",
  NotDetected: "未发现旧数据",
  Completed: "已完成",
  Partial: "部分完成",
  Required: "失败，当前处于只读诊断模式",
};

const MIGRATION_STATE_TONES: Record<MigrationStateDto, string> = {
  NotConfigured: "neutral",
  NotDetected: "neutral",
  Completed: "success",
  Partial: "warning",
  Required: "danger",
};

function migrationStateDescription(migration: LegacyMigrationDto): string {
  switch (migration.state) {
    case "NotConfigured":
      return "未配置旧版迁移来源，当前没有需要导入的数据。";
    case "NotDetected":
      return "已检查常见旧版安装位置，未发现可迁移的数据。";
    case "Completed":
      return "旧数据已导入，应用可以继续正常写入。";
    case "Partial":
      return "迁移已完成，但部分损坏记录未导入。请保留旧版数据以便核对。";
    case "Required":
      return "旧数据迁移未完成，运行时已暂停写入，只允许查看诊断和处理迁移问题。";
  }
}

function migrationStateLabel(migration: LegacyMigrationDto): string {
  if (migration.state !== "Partial") {
    return MIGRATION_STATE_LABELS[migration.state];
  }
  const skippedRecords = migration.report?.skippedRecords;
  return skippedRecords === undefined
    ? "部分完成"
    : `部分完成，跳过 ${skippedRecords} 条损坏记录`;
}

function safeDiagnosticsText(diagnostics: DiagnosticsDto): string {
  return JSON.stringify(
    {
      generatedAt: diagnostics.generatedAt,
      runtime: diagnostics.runtime,
      storage: diagnostics.storage,
      components: diagnostics.components,
      items: diagnostics.items,
      migration: diagnostics.migration,
    },
    null,
    2,
  );
}

function MigrationIssueDetails({ issue }: { issue: MigrationIssueDto }) {
  return (
    <div className="migration-issue" role="group" aria-label="迁移失败详情">
      <h3>失败详情</h3>
      <dl className="migration-issue-list">
        <div>
          <dt>文件</dt>
          <dd>{issue.file ?? "未提供"}</dd>
        </div>
        <div>
          <dt>字段</dt>
          <dd>{issue.field ?? "未提供"}</dd>
        </div>
        <div>
          <dt>错误</dt>
          <dd>{issue.message}</dd>
        </div>
      </dl>
      <p className="migration-backup-advice">
        备份建议：请先将旧版数据目录和现有文件完整复制到安全位置，再处理迁移问题并重新检测。迁移完成前请保留原文件。
      </p>
    </div>
  );
}

function MigrationReportDetails({ report }: { report: MigrationReportDto }) {
  return (
    <div className="migration-report" id="migration-report-details">
      <dl className="migration-report-list">
        <div>
          <dt>导入时间</dt>
          <dd>{report.importedAt ?? "未提供"}</dd>
        </div>
        <div>
          <dt>源文件</dt>
          <dd>{report.sourceFileCount}</dd>
        </div>
        <div>
          <dt>设置</dt>
          <dd>{report.settingsImported}</dd>
        </div>
        <div>
          <dt>Agent 配置</dt>
          <dd>{report.agentConfigsImported}</dd>
        </div>
        <div>
          <dt>账号</dt>
          <dd>{report.accountsImported}</dd>
        </div>
        <div>
          <dt>通知 / 投递</dt>
          <dd>
            {report.notificationsImported} / {report.deliveriesImported}
          </dd>
        </div>
        <div>
          <dt>路由 / Claim</dt>
          <dd>
            {report.routesImported} / {report.claimsImported}
          </dd>
        </div>
        <div>
          <dt>跳过损坏记录</dt>
          <dd>{report.skippedRecords}</dd>
        </div>
      </dl>

      {report.warnings.length === 0 ? (
        <p className="migration-report-empty">报告未记录损坏警告。</p>
      ) : (
        <ul className="migration-warning-list" aria-label="迁移损坏警告">
          {report.warnings.map((warning, index) => (
            <li key={`${warning.code}-${warning.file}-${String(index)}`}>
              <code>{warning.code}</code>
              <span>{warning.file}</span>
              {warning.record === null ? null : (
                <span>第 {warning.record} 条</span>
              )}
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}

function MigrationDiagnostics({
  bridge,
  migration,
}: {
  bridge: HostBridge;
  migration: LegacyMigrationDto;
}) {
  const queryClient = useQueryClient();
  const retryMutation = useDiagnosticActionMutation(
    bridge,
    "retry_legacy_migration",
  );
  const [showReport, setShowReport] = useState(false);
  const [retryError, setRetryError] = useState<unknown>(null);
  const retryErrorView = retryError ? toUserError(retryError) : null;

  const retryMigration = async () => {
    setRetryError(null);
    try {
      await retryMutation.mutateAsync({});
    } catch (error) {
      setRetryError(error);
      await Promise.all([
        queryClient.invalidateQueries({ queryKey: queryKeys.diagnostics() }),
        queryClient.invalidateQueries({ queryKey: queryKeys.snapshot() }),
      ]);
    }
  };

  return (
    <div className="migration-diagnostics">
      <div
        className={`migration-status migration-status--${MIGRATION_STATE_TONES[migration.state]}`}
      >
        <strong>{migrationStateLabel(migration)}</strong>
        <p>{migrationStateDescription(migration)}</p>
        <p className="migration-source-state">
          旧数据来源：
          {migration.sourceDetected ? "已检测到" : "未检测到"}
        </p>
      </div>

      {migration.reportFile ? (
        <p className="migration-report-file">
          迁移报告文件：<code>{migration.reportFile}</code>
        </p>
      ) : null}

      {migration.error ? (
        <MigrationIssueDetails issue={migration.error} />
      ) : null}

      <div className="migration-actions">
        <button
          className="button button-secondary"
          type="button"
          aria-controls="migration-report-details"
          aria-expanded={showReport}
          disabled={migration.report === null}
          onClick={() => setShowReport((current) => !current)}
        >
          <FileText aria-hidden="true" size={15} />
          查看迁移报告
        </button>
        {migration.report === null ? (
          <span className="migration-report-unavailable">暂无报告</span>
        ) : null}
        <button
          className="button button-secondary"
          type="button"
          disabled={retryMutation.isPending}
          onClick={() => void retryMigration()}
        >
          <RefreshCw aria-hidden="true" size={15} />
          {retryMutation.isPending ? "正在重新检测" : "重新检测"}
        </button>
      </div>

      {retryErrorView ? (
        <InlineError
          title="重新检测失败"
          message={retryErrorView.message}
        />
      ) : null}

      {showReport && migration.report ? (
        <MigrationReportDetails report={migration.report} />
      ) : null}
    </div>
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
              className="diagnostics-section"
              aria-labelledby="diagnostics-migration-title"
            >
              <header className="section-heading">
                <div>
                  <h2 id="diagnostics-migration-title">旧数据迁移</h2>
                  <p className="section-description">
                    迁移状态来自运行时快照；失败时仅提供查看报告和重新检测。
                  </p>
                </div>
              </header>
              <MigrationDiagnostics
                bridge={bridge}
                migration={diagnostics.migration}
              />
            </section>

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
