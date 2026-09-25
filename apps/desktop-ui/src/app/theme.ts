import { useCallback, useEffect, useState } from "react";

export type ThemePreference = "dark" | "light" | "system";
export type ResolvedTheme = "dark" | "light";

/** localStorage 记忆键：只存偏好（dark/light/system），解析结果不落盘。 */
export const THEME_STORAGE_KEY = "agentnotify.theme";

const DARK_MEDIA_QUERY = "(prefers-color-scheme: dark)";

function getDarkMediaQuery(): MediaQueryList | null {
  if (
    typeof window === "undefined" ||
    typeof window.matchMedia !== "function"
  ) {
    return null;
  }
  return window.matchMedia(DARK_MEDIA_QUERY);
}

/** 读取主题偏好；无存储或存储值非法时回退 system（跟随系统）。 */
export function readPreference(): ThemePreference {
  if (typeof window === "undefined") {
    return "system";
  }
  try {
    const raw = window.localStorage.getItem(THEME_STORAGE_KEY);
    if (raw === "dark" || raw === "light" || raw === "system") {
      return raw;
    }
  } catch {
    // 隐私模式等 localStorage 不可用时视为未设置
  }
  return "system";
}

/** 把偏好解析为实际主题；system 跟随 prefers-color-scheme，无法查询时兜底 dark。 */
export function resolveTheme(preference: ThemePreference): ResolvedTheme {
  if (preference === "dark" || preference === "light") {
    return preference;
  }
  const media = getDarkMediaQuery();
  if (!media) {
    return "dark";
  }
  return media.matches ? "dark" : "light";
}

/** 应用主题：写 document.documentElement.dataset.theme，返回解析结果。 */
export function applyTheme(preference: ThemePreference): ResolvedTheme {
  const resolved = resolveTheme(preference);
  if (typeof document !== "undefined") {
    document.documentElement.dataset.theme = resolved;
  }
  if (
    typeof window !== "undefined" &&
    ("__TAURI_INTERNALS__" in window || "__TAURI__" in window)
  ) {
    try {
      import("@tauri-apps/api/app")
        .then(({ setTheme }) => {
          void setTheme(resolved).catch(() => {});
        })
        .catch(() => {});
      import("@tauri-apps/api/webviewWindow")
        .then(({ getCurrentWebviewWindow }) => {
          void getCurrentWebviewWindow().setTheme(resolved).catch(() => {});
        })
        .catch(() => {});
    } catch {}
  }
  return resolved;
}

// 模块加载时立即应用已记忆主题，尽早对齐原生标题栏明暗，避免首屏闪烁
if (typeof window !== "undefined") {
  try {
    applyTheme(readPreference());
  } catch {}
}

/** system 偏好下监听系统明暗变化；返回取消订阅函数。 */
export function subscribeSystemTheme(callback: () => void): () => void {
  const media = getDarkMediaQuery();
  if (!media || typeof media.addEventListener !== "function") {
    return () => undefined;
  }
  media.addEventListener("change", callback);
  return () => media.removeEventListener("change", callback);
}

export interface ThemeController {
  preference: ThemePreference;
  resolvedTheme: ResolvedTheme;
  setPreference: (next: ThemePreference) => void;
}

/** React 侧主题控制器：读取偏好、切换并记忆、system 下跟随系统变化。 */
export function useTheme(): ThemeController {
  const [preference, setPreferenceState] = useState<ThemePreference>(() =>
    readPreference(),
  );
  const [resolvedTheme, setResolvedTheme] = useState<ResolvedTheme>(() =>
    resolveTheme(readPreference()),
  );

  const setPreference = useCallback((next: ThemePreference) => {
    setPreferenceState(next);
    try {
      window.localStorage.setItem(THEME_STORAGE_KEY, next);
    } catch {
      // 存储不可用时仍切换当前会话主题
    }
    setResolvedTheme(applyTheme(next));
  }, []);

  useEffect(() => {
    setResolvedTheme(applyTheme(preference));
    if (preference !== "system") {
      return undefined;
    }
    return subscribeSystemTheme(() => {
      setResolvedTheme(applyTheme("system"));
    });
  }, [preference]);

  return { preference, resolvedTheme, setPreference };
}
