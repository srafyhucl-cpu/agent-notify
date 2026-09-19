import type { SettingsDto, UpdateChannelDto } from "../../bridge/types";

const UPDATE_CHANNEL_LABELS: Record<UpdateChannelDto, string> = {
  Stable: "稳定版",
  Beta: "测试版",
};

export interface UpdateSettingsProps {
  value: Pick<SettingsDto, "autoStart" | "startHidden" | "updateChannel">;
  disabled?: boolean;
  onChange: (
    patch: Partial<Pick<SettingsDto, "autoStart" | "startHidden" | "updateChannel">>,
  ) => void;
}

export function UpdateSettings({
  value,
  disabled = false,
  onChange,
}: UpdateSettingsProps) {
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
          <small>当前版本未提供稳定的检查、安装或回滚命令。</small>
        </span>
        <button className="button button-secondary" type="button" disabled>
          当前版本不可用
        </button>
      </div>
    </div>
  );
}
