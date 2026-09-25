import { useCallback, useEffect, useState } from "react";

export const ACCOUNT_NAMES_STORAGE_KEY = "agentnotify.account_names";
const UPDATE_EVENT_NAME = "agentnotify:account-names-updated";

export function getAccountCustomNames(): Record<string, string> {
  if (typeof window === "undefined") {
    return {};
  }
  try {
    const raw = window.localStorage.getItem(ACCOUNT_NAMES_STORAGE_KEY);
    if (!raw) {
      return {};
    }
    const parsed = JSON.parse(raw);
    if (parsed && typeof parsed === "object") {
      return parsed as Record<string, string>;
    }
  } catch {
    // 忽略异常，降级空对象
  }
  return {};
}

export function getAccountCustomName(accountId: string): string | null {
  const names = getAccountCustomNames();
  return names[accountId] ?? null;
}

export function setAccountCustomName(accountId: string, name: string): void {
  if (typeof window === "undefined" || !accountId) {
    return;
  }
  try {
    const trimmed = name.trim();
    const names = getAccountCustomNames();
    if (trimmed === "") {
      delete names[accountId];
    } else {
      names[accountId] = trimmed;
    }
    window.localStorage.setItem(
      ACCOUNT_NAMES_STORAGE_KEY,
      JSON.stringify(names),
    );
    window.dispatchEvent(new CustomEvent(UPDATE_EVENT_NAME));
  } catch {
    // 忽略存储写入限制
  }
}

/**
 * 将生僻的内部 raw accountId（如 clawbot-51e112ee60821be3）转换为人类可读名称，
 * 避免在未命名或老数据中直接裸露冗长哈希。
 */
export function humanizeRawAccountId(accountId: string): string {
  if (!accountId) {
    return "未命名账号";
  }
  if (accountId.startsWith("clawbot-")) {
    const raw = accountId.replace(/^clawbot-/, "");
    return `微信账号 (${raw.slice(0, 6)})`;
  }
  return accountId;
}

/**
 * 格式化账号展示名：
 * 1. 优先使用用户自定义命名（有意义的命名，如「工作微信」「通知群」等）
 * 2. 其次使用后端返回的 displayName
 * 3. 兜底人性化账号 ID
 */
export function getAccountDisplayName(account: {
  id: string;
  displayName?: string | null;
}): string {
  const custom = getAccountCustomName(account.id);
  if (custom && custom.trim() !== "") {
    return custom;
  }
  if (account.displayName && account.displayName.trim() !== "") {
    return account.displayName;
  }
  return humanizeRawAccountId(account.id);
}

export function formatAccountName(
  accountId: string,
  fallbackDisplayName?: string | null,
): string {
  const custom = getAccountCustomName(accountId);
  if (custom && custom.trim() !== "") {
    return custom;
  }
  if (fallbackDisplayName && fallbackDisplayName.trim() !== "") {
    return fallbackDisplayName;
  }
  return humanizeRawAccountId(accountId);
}

export function formatChannelAccountOptionLabel(
  channelName: string,
  accountName: string,
): string {
  const cName = (channelName ?? "").trim();
  const aName = (accountName ?? "").trim();
  if (!aName) return cName;
  if (!cName) return aName;
  if (aName.startsWith(cName)) {
    return aName;
  }
  return `${cName} · ${aName}`;
}

export function useAccountNames() {
  const [names, setNames] = useState<Record<string, string>>(() =>
    getAccountCustomNames(),
  );

  useEffect(() => {
    const handleUpdate = () => {
      setNames(getAccountCustomNames());
    };
    window.addEventListener(UPDATE_EVENT_NAME, handleUpdate);
    window.addEventListener("storage", handleUpdate);
    return () => {
      window.removeEventListener(UPDATE_EVENT_NAME, handleUpdate);
      window.removeEventListener("storage", handleUpdate);
    };
  }, []);

  const getDisplayName = useCallback(
    (account: { id: string; displayName?: string | null }) => {
      const custom = names[account.id];
      if (custom && custom.trim() !== "") {
        return custom;
      }
      if (account.displayName && account.displayName.trim() !== "") {
        return account.displayName;
      }
      return humanizeRawAccountId(account.id);
    },
    [names],
  );

  const formatAccount = useCallback(
    (accountId: string, fallbackDisplayName?: string | null) => {
      const custom = names[accountId];
      if (custom && custom.trim() !== "") {
        return custom;
      }
      if (fallbackDisplayName && fallbackDisplayName.trim() !== "") {
        return fallbackDisplayName;
      }
      return humanizeRawAccountId(accountId);
    },
    [names],
  );

  const setCustomName = useCallback((accountId: string, name: string) => {
    setAccountCustomName(accountId, name);
  }, []);

  return {
    names,
    getDisplayName,
    formatAccount,
    setCustomName,
  };
}
