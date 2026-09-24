import type { QuietHoursDto } from "../../bridge/types";
import { FieldRow } from "../../components/patterns";

const DEFAULT_QUIET_HOURS: QuietHoursDto = {
  enabled: true,
  start: "22:00",
  end: "07:00",
};

export interface QuietHoursFormProps {
  value: QuietHoursDto | null;
  disabled?: boolean;
  onChange: (value: QuietHoursDto | null) => void;
}

export function QuietHoursForm({
  value,
  disabled = false,
  onChange,
}: QuietHoursFormProps) {
  const enabled = value?.enabled === true;

  return (
    <>
      <FieldRow
        label="勿扰时段"
        description="启用后在设定时段内不发送通知"
        control={
          <input
            type="checkbox"
            role="switch"
            aria-label="启用勿扰时段"
            checked={enabled}
            disabled={disabled}
            onChange={(event) =>
              onChange(
                event.currentTarget.checked
                  ? value
                    ? { ...value, enabled: true }
                    : DEFAULT_QUIET_HOURS
                  : null,
              )
            }
          />
        }
      />

      <FieldRow
        label="勿扰时间"
        description="开始与结束时间；跨越午夜时按次日结束。"
        control={
          <div className="settings-inline-fields">
            <label>
              <span>开始</span>
              <input
                aria-label="勿扰开始"
                type="time"
                value={value?.start ?? "22:00"}
                disabled={disabled || !enabled}
                onChange={(event) =>
                  onChange({
                    enabled: true,
                    start: event.currentTarget.value,
                    end: value?.end ?? "07:00",
                  })
                }
              />
            </label>
            <label>
              <span>结束</span>
              <input
                aria-label="勿扰结束"
                type="time"
                value={value?.end ?? "07:00"}
                disabled={disabled || !enabled}
                onChange={(event) =>
                  onChange({
                    enabled: true,
                    start: value?.start ?? "22:00",
                    end: event.currentTarget.value,
                  })
                }
              />
            </label>
          </div>
        }
      />
    </>
  );
}
