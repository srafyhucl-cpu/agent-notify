import { Download, RefreshCw, ShieldAlert, ShieldCheck } from "lucide-react";
import { useState } from "react";

import type { HostBridge } from "../../bridge";
import type {
  InstallUpdateResultDto,
  SettingsDto,
  UpdateChannelDto,
  UpdateStateDto,
  UpdateStatusDto,
} from "../../bridge/types";
import { InlineError } from "../../components/InlineError";
import { FieldRow } from "../../components/patterns";
import { toUserError } from "../../data/errors";

const UPDATE_CHANNEL_LABELS: Record<UpdateChannelDto, string> = {
  Stable: "稳定版",
  Beta: "测试版",
};

/** 只有这两个状态允许一键下载并安装；其余状态保持禁用并说明原因。 */
type InstallableState = Extract<UpdateStateDto, "Available" | "ReadyToInstall">;

function isInstallableState(state: UpdateStateDto): state is InstallableState {
  return state === "Available" || state === "ReadyToInstall";
}

const INSTALL_REASON_ID = "settings-update-install-reason";

const INSTALL_DISABLED_REASONS: Record<
  Exclude<UpdateStateDto, InstallableState>,
  string
> = {
  UpToDate: "当前已是最新版本，无需下载安装。",
  Unsupported: "当前环境或更新通道不支持在线安装。",
  Failed: "上次检查更新失败，请先重新检查。",
};

/** 未安装过时按检查结果说明；安装失败过时优先说明安装失败。 */
function installDisabledReason(
  status: UpdateStatusDto | null,
  installResult: InstallUpdateResultDto | null,
): string | null {
  if (installResult && isInstallableState(installResult.state)) {
    return null;
  }
  if (installResult?.state === "Failed") {
    return "上次安装未成功，请重新检查更新后再试。";
  }
  if (!status) {
    return "先检查更新，确认存在可安装版本后才能下载。";
  }
  if (isInstallableState(status.state)) {
    return null;
  }
  return INSTALL_DISABLED_REASONS[status.state];
}

function InstallResult({ result }: { result: InstallUpdateResultDto }) {
  if (result.state === "Failed") {
    return <InlineError title="更新安装失败" message={result.message} />;
  }
  if (result.state === "ReadyToInstall") {
    return (
      <p className="settings-feedback settings-feedback--success" role="status">
        安装已就绪：{result.message}
      </p>
    );
  }
  return <p className="settings-feedback">{result.message}</p>;
}

export interface UpdateSettingsProps {
  bridge: HostBridge;
  value: Pick<SettingsDto, "autoStart" | "startHidden" | "updateChannel">;
  disabled?: boolean;
  onChange: (
    patch: Partial<Pick<SettingsDto, "autoStart" | "startHidden" | "updateChannel">>,
  ) => void;
}

export function UpdateSettings({
  bridge,
  value,
  disabled = false,
  onChange,
}: UpdateSettingsProps) {
  const [status, setStatus] = useState<UpdateStatusDto | null>(null);
  const [checking, setChecking] = useState(false);
  const [installing, setInstalling] = useState(false);
  const [installResult, setInstallResult] =
    useState<InstallUpdateResultDto | null>(null);
  const [errorMessage, setErrorMessage] = useState<string | null>(null);

  const checkForUpdate = async () => {
    setChecking(true);
    setErrorMessage(null);
    // 重新检查后旧安装结果不再可信，先清空避免误导。
    setInstallResult(null);
    try {
      setStatus(await bridge.invoke("get_update_status", {}));
    } catch (error) {
      setErrorMessage(toUserError(error).message);
    } finally {
      setChecking(false);
    }
  };

  const installUpdate = async () => {
    setInstalling(true);
    setErrorMessage(null);
    try {
      setInstallResult(await bridge.invoke("install_update", {}));
    } catch (error) {
      setErrorMessage(toUserError(error).message);
    } finally {
      setInstalling(false);
    }
  };

  const blockedReason = installDisabledReason(status, installResult);
  const installDisabled = disabled || checking || installing || blockedReason !== null;

  return (
    <div className="settings-fields">
      <FieldRow
        label="随系统启动"
        description="登录 Windows 后自动启动 AgentNotify"
        control={
          <input
            type="checkbox"
            role="switch"
            aria-label="随系统启动"
            checked={value.autoStart}
            disabled={disabled}
            onChange={(event) =>
              onChange({ autoStart: event.currentTarget.checked })
            }
          />
        }
      />

      <FieldRow
        label="启动时隐藏"
        description="启动后仅在系统托盘显示"
        control={
          <input
            type="checkbox"
            role="switch"
            aria-label="启动时隐藏"
            checked={value.startHidden}
            disabled={disabled}
            onChange={(event) =>
              onChange({ startHidden: event.currentTarget.checked })
            }
          />
        }
      />

      <FieldRow
        label="更新通道"
        description="稳定版用于日常使用，测试版用于提前验证。"
        control={
          <select
            className="settings-control"
            aria-label="更新通道"
            value={value.updateChannel}
            disabled={disabled}
            onChange={(event) =>
              onChange({
                updateChannel: event.currentTarget.value as UpdateChannelDto,
              })
            }
          >
            {Object.entries(UPDATE_CHANNEL_LABELS).map(([channel, label]) => (
              <option value={channel} key={channel}>
                {label}
              </option>
            ))}
          </select>
        }
      />

      <div className="settings-unavailable-row">
        <span>
          <strong>检查与安装更新</strong>
          <small>
            先检查发布通道；安装动作仍由宿主校验包来源、版本和签名。
          </small>
        </span>
        <div className="settings-update-actions">
          <button
            className="button button-secondary"
            type="button"
            disabled={disabled || checking || installing}
            onClick={() => void checkForUpdate()}
          >
            <RefreshCw aria-hidden="true" size={15} />
            {checking ? "正在检查" : "检查更新"}
          </button>
          <button
            className="button"
            type="button"
            disabled={installDisabled}
            aria-describedby={blockedReason ? INSTALL_REASON_ID : undefined}
            onClick={() => void installUpdate()}
          >
            <Download aria-hidden="true" size={15} />
            {installing ? "正在下载并安装" : "下载并安装"}
          </button>
        </div>
      </div>

      {blockedReason ? (
        <p className="settings-feedback" id={INSTALL_REASON_ID}>
          {blockedReason}
        </p>
      ) : null}

      {installing ? (
        <p className="settings-feedback" role="status">
          正在安装新版本，完成后应用会自动重启，请保持应用运行。
        </p>
      ) : null}

      {status ? (
        <div className="settings-subsection" role="status">
          <div className="settings-subsection-heading">
            <strong>
              {status.preview ? "Rust 预览通道" : `当前版本 ${status.currentVersion}`}
            </strong>
            <small>{status.message}</small>
          </div>
          <p className="settings-feedback">
            {status.signed ? (
              <ShieldCheck aria-hidden="true" size={15} />
            ) : (
              <ShieldAlert aria-hidden="true" size={15} />
            )}
            {status.preview && !status.signed
              ? "测试包未签名，仅供内部验证；正式版不会安装未签名更新。"
              : status.signed
                ? "更新包签名已通过宿主校验。"
                : "正式通道将拒绝未签名更新包。"}
          </p>
        </div>
      ) : null}

      {installResult ? <InstallResult result={installResult} /> : null}

      {errorMessage ? (
        <p className="settings-feedback settings-feedback--error" role="alert">
          {errorMessage}
        </p>
      ) : null}
    </div>
  );
}
