import type { SettingsDto } from "../../bridge/types";
import { FieldRow } from "../../components/patterns";

export interface ReplySettingsFormProps {
  value: Pick<
    SettingsDto,
    "replyEnabled" | "deliveryReceiptEnabled" | "routeTtlSeconds"
  >;
  disabled?: boolean;
  onChange: (
    patch: Partial<
      Pick<
        SettingsDto,
        "replyEnabled" | "deliveryReceiptEnabled" | "routeTtlSeconds"
      >
    >,
  ) => void;
}

export function ReplySettingsForm({
  value,
  disabled = false,
  onChange,
}: ReplySettingsFormProps) {
  return (
    <div className="settings-fields">
      <FieldRow
        label="引用回复"
        description="允许通过通知回复 Agent 会话"
        control={
          <input
            type="checkbox"
            role="switch"
            aria-label="引用回复"
            checked={value.replyEnabled}
            disabled={disabled}
            onChange={(event) =>
              onChange({ replyEnabled: event.currentTarget.checked })
            }
          />
        }
      />

      <FieldRow
        label="送达确认"
        description="回复处理完成后尝试更新送达状态"
        control={
          <input
            type="checkbox"
            role="switch"
            aria-label="送达确认"
            checked={value.deliveryReceiptEnabled}
            disabled={disabled || !value.replyEnabled}
            onChange={(event) =>
              onChange({ deliveryReceiptEnabled: event.currentTarget.checked })
            }
          />
        }
      />

      <FieldRow
        label="路由有效期（秒）"
        description="允许 60 到 604800 秒。"
        control={
          <input
            className="settings-control"
            aria-label="路由有效期（秒）"
            type="number"
            min={60}
            max={604800}
            step={1}
            value={value.routeTtlSeconds}
            disabled={disabled || !value.replyEnabled}
            onChange={(event) =>
              onChange({
                routeTtlSeconds: Number.parseInt(event.currentTarget.value, 10),
              })
            }
          />
        }
      />
    </div>
  );
}
