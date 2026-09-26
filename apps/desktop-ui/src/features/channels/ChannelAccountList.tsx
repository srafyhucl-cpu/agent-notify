import { Check, ChevronDown, Pencil, Trash2, X } from "lucide-react";
import { Fragment, useState, type ReactNode } from "react";

import type {
  ChannelAccountDto,
  ChannelDto,
} from "../../bridge/types";
import { StatusBadge, type StatusBadgeTone } from "../../components/patterns";
import { useAccountNames } from "../../data/accountNames";

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
  onDelete: (account: ChannelAccountDto) => void;
  renderDetail?: (entry: ChannelAccountEntry) => ReactNode;
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
  onDelete,
  renderDetail,
}: ChannelAccountListProps) {
  const { getDisplayName, setCustomName } = useAccountNames();
  const [editingAccountId, setEditingAccountId] = useState<string | null>(null);
  const [editingName, setEditingName] = useState("");

  const startRename = (account: ChannelAccountDto) => {
    setEditingAccountId(account.id);
    setEditingName(getDisplayName(account));
  };

  const saveRename = (accountId: string) => {
    setCustomName(accountId, editingName);
    setEditingAccountId(null);
  };

  const cancelRename = () => {
    setEditingAccountId(null);
  };

  return (
    <div className="channel-account-list">
      <table>
        <caption className="visually-hidden">渠道账号状态与操作</caption>
        <thead>
          <tr>
            <th scope="col">账号</th>
            <th scope="col">状态</th>
            <th scope="col">启用</th>
            <th scope="col" className="channel-th-action">
              <span className="visually-hidden">操作</span>
            </th>
            <th scope="col" className="channel-th-expand">
              <span className="visually-hidden">展开</span>
            </th>
          </tr>
        </thead>
        <tbody>
          {entries.map((entry) => {
            const { account } = entry;
            const status = accountStatus(account);
            const isSelected = account.id === selectedAccountId;
            const isEditing = editingAccountId === account.id;
            const displayName = getDisplayName(account);

            return (
              <Fragment key={account.id}>
                <tr
                  className={`channel-account-row ${
                    isSelected ? "channel-account-row--selected" : ""
                  }`}
                  onClick={() => onSelect(isSelected ? "" : account.id)}
                >
                  <th scope="row">
                    {isEditing ? (
                      <div
                        className="account-inline-rename"
                        onClick={(e) => e.stopPropagation()}
                      >
                        <input
                          className="rename-input"
                          type="text"
                          aria-label="账号名称"
                          value={editingName}
                          onChange={(e) => setEditingName(e.currentTarget.value)}
                          onKeyDown={(e) => {
                            if (e.key === "Enter") saveRename(account.id);
                            if (e.key === "Escape") cancelRename();
                          }}
                          autoFocus
                        />
                        <button
                          className="button-icon-subtle button-icon-confirm"
                          type="button"
                          aria-label="保存名称"
                          onClick={() => saveRename(account.id)}
                        >
                          <Check size={14} />
                        </button>
                        <button
                          className="button-icon-subtle"
                          type="button"
                          aria-label="取消"
                          onClick={cancelRename}
                        >
                          <X size={14} />
                        </button>
                      </div>
                    ) : (
                    <div className="account-title-group">
                      <button
                        className="channel-account-name"
                        type="button"
                        aria-expanded={isSelected}
                        aria-controls={
                          isSelected ? `channel-detail-${account.id}` : undefined
                        }
                        onClick={() => onSelect(isSelected ? "" : account.id)}
                      >
                        <span className="channel-account-name-text">
                          {displayName}
                        </span>
                      </button>
                      <button
                        className="rename-trigger-btn"
                        type="button"
                        title="自定义命名"
                        aria-label={`重命名账号 ${displayName}`}
                        onClick={(e) => {
                          e.stopPropagation();
                          startRename(account);
                        }}
                      >
                        <Pencil size={12} aria-hidden="true" />
                      </button>
                    </div>
                  )}
                </th>
                <td>
                  <span title={account.health.detail?.message ?? undefined}>
                    <StatusBadge tone={status.tone}>{status.label}</StatusBadge>
                  </span>
                  {account.health.detail ? (
                    <span className="sr-only">
                      {account.health.detail.message}
                    </span>
                  ) : null}
                </td>
                  <td>
                    <label
                      className="switch-control"
                      onClick={(e) => e.stopPropagation()}
                    >
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
                  <td
                    className="channel-account-action-cell"
                    onClick={(e) => e.stopPropagation()}
                  >
                    <button
                      className="button-icon-subtle button-icon-danger"
                      type="button"
                      title="删除账号"
                      aria-label={`退出账号 ${displayName}`}
                      disabled={pendingAccountId === account.id}
                      onClick={() => onDelete(account)}
                    >
                      <Trash2 size={13} aria-hidden="true" />
                    </button>
                  </td>
                  <td className="channel-account-expand-cell">
                    <ChevronDown
                      className={`channel-chevron ${isSelected ? "channel-chevron--open" : ""}`}
                      aria-hidden="true"
                      size={16}
                    />
                  </td>
                </tr>

                {isSelected && renderDetail ? (
                  <tr className="channel-account-detail-row">
                    <td colSpan={5}>
                      <div
                        className="channel-account-inline-detail"
                        id={`channel-detail-${account.id}`}
                      >
                        {renderDetail(entry)}
                      </div>
                    </td>
                  </tr>
                ) : null}
              </Fragment>
            );
          })}
        </tbody>
      </table>
    </div>
  );
}
