import { Settings2 } from "lucide-react";

import type {
  ChannelAccountDto,
  ChannelDto,
} from "../../bridge/types";
import { StatusBadge, type StatusBadgeTone } from "../../components/patterns";

export interface ChannelAccountEntry {
  channel: ChannelDto;
  account: ChannelAccountDto;
}

export interface ChannelAccountListProps {
  entries: ChannelAccountEntry[];
  selectedAccountId: string | null;
  pendingAccountId: string | null;
  onSelect: (accountId: string) => void;
  onToggle: (account: ChannelAccountDto, enabled: boolean) => void;
}

function accountStatus(account: ChannelAccountDto): {
  label: string;
  tone: StatusBadgeTone;
} {
  if (!account.enabled) {
    return { label: "已停用", tone: "warning" };
  }
  if (account.health.stale) {
    return { label: "状态已过期", tone: "danger" };
  }
  if (!account.health.available) {
    return { label: "登录异常", tone: "danger" };
  }
  return { label: "服务正常", tone: "success" };
}

export function ChannelAccountList({
  entries,
  selectedAccountId,
  pendingAccountId,
  onSelect,
  onToggle,
}: ChannelAccountListProps) {
  return (
    <div className="channel-account-list">
      <table>
        <caption className="visually-hidden">渠道账号状态与操作</caption>
        <thead>
          <tr>
            <th scope="col">账号</th>
            <th scope="col">状态</th>
            <th scope="col">启用</th>
            <th scope="col">详情</th>
          </tr>
        </thead>
        <tbody>
          {entries.map(({ account }) => {
            const status = accountStatus(account);
            return (
              <tr
                className={
                  account.id === selectedAccountId
                    ? "channel-account-row--selected"
                    : undefined
                }
                key={account.id}
              >
                <th scope="row">
                  <button
                    className="channel-account-name"
                    type="button"
                    aria-pressed={account.id === selectedAccountId}
                    onClick={() => onSelect(account.id)}
                  >
                    <span className="channel-account-name-text">
                      {account.displayName}
                    </span>
                  </button>
                </th>
                <td>
                  <StatusBadge tone={status.tone}>{status.label}</StatusBadge>
                  {account.health.detail ? (
                    <span className="channel-account-error">
                      {account.health.detail.message}
                    </span>
                  ) : null}
                </td>
                <td>
                  <label className="switch-control">
                    <input
                      type="checkbox"
                      role="switch"
                      aria-label={`${account.displayName} 启用`}
                      checked={account.enabled}
                      disabled={pendingAccountId === account.id}
                      onChange={(event) =>
                        onToggle(account, event.currentTarget.checked)
                      }
                    />
                    <span className="switch-track" aria-hidden="true" />
                  </label>
                </td>
                <td>
                  <button
                    className="icon-text-button"
                    type="button"
                    aria-label={`查看账号 ${account.displayName}`}
                    onClick={() => onSelect(account.id)}
                  >
                    <Settings2 aria-hidden="true" size={15} />
                    查看
                  </button>
                </td>
              </tr>
            );
          })}
        </tbody>
      </table>
    </div>
  );
}
