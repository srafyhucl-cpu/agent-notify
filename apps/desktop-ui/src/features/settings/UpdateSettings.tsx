import { RefreshCw, ShieldAlert, ShieldCheck } from "lucide-react";
import { useState } from "react";

import type { HostBridge } from "../../bridge";
import type {
  SettingsDto,
  UpdateChannelDto,
  UpdateStatusDto,
} from "../../bridge/types";
import { toUserError } from "../../data/errors";

const UPDATE_CHANNEL_LABELS: Record<UpdateChannelDto, string> = {
  Stable: "稳定版",
  Beta: "测试版",
};

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
  const [errorMessage, setErrorMessage] = useState<string | null>(null);

  const checkForUpdate = async () => {
    setChecking(true);
    setErrorMessage(null);
    try {
      setStatus(await bridge.invoke("get_update_status", {}));
    } catch (error) {
      setErrorMessage(toUserError(error).message);
    } finally {
      setChecking(false);
    }
  };

  return (
    <div className="settings-fields">
      <label className="settings-switch-row">
        <span>
          <strong>随系统启动</strong>
          <small>登录 Windows 后自动启动 AgentNotify</small>
        </span>
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
      </label>

      <label className="settings-switch-row">
        <span>
          <strong>启动时隐藏</strong>
          <small>启动后仅在系统托盘显示</small>
        </span>
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
      </label>

      <label className="settings-field">
        <span>更新通道</span>
        <select
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
      </label>

      <div className="settings-unavailable-row">
        <span>
          <strong>检查与安装更新</strong>
          <small>
            先检查发布通道；安装动作仍由宿主校验包来源、版本和签名。
          </small>
        </span>
        <button
          className="button button-secondary"
          type="button"
          disabled={disabled || checking}
          onClick={() => void checkForUpdate()}
        >
          <RefreshCw aria-hidden="true" size={15} />
          {checking ? "正在检查" : "检查更新"}
        </button>
      </div>

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

      {errorMessage ? (
        <p className="settings-feedback settings-feedback--error" role="alert">
          {errorMessage}
        </p>
      ) : null}
    </div>
  );
}
