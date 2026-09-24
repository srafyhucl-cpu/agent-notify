import { Monitor, Moon, Sun } from "lucide-react";
import type { LucideIcon } from "lucide-react";
import { useRef, type KeyboardEvent } from "react";

import { useTheme, type ThemePreference } from "../app/theme";

interface ThemeOption {
  value: ThemePreference;
  /** 可访问名（完整语义） */
  label: string;
  /** 可见短标签（窄栏不换行、不溢出） */
  shortLabel: string;
  icon: LucideIcon;
}

const THEME_OPTIONS: readonly ThemeOption[] = [
  { value: "light", label: "明", shortLabel: "明", icon: Sun },
  { value: "dark", label: "暗", shortLabel: "暗", icon: Moon },
  { value: "system", label: "跟随系统", shortLabel: "系统", icon: Monitor },
] as const;

/**
 * 导航底部主题切换：明 / 暗 / 跟随系统。
 * 语义采用 radiogroup + roving tabindex：Tab 进入当前选中项，←/→ 或 ↑/↓ 循环切换。
 * 选它而非 aria-pressed 按钮组，是因为单选语义更贴合「三选一」，且方向键导航是
 * radiogroup 的标准预期，axe 与屏幕阅读器支持最稳。
 */
export function ThemeSwitcher() {
  const { preference, setPreference } = useTheme();
  const optionRefs = useRef<Array<HTMLButtonElement | null>>([]);
  const selectedIndex = Math.max(
    0,
    THEME_OPTIONS.findIndex((option) => option.value === preference),
  );

  const selectAt = (index: number) => {
    const option = THEME_OPTIONS[index];
    if (!option) {
      return;
    }
    setPreference(option.value);
    optionRefs.current[index]?.focus();
  };

  const handleKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    const lastIndex = THEME_OPTIONS.length - 1;
    let nextIndex: number | null = null;
    switch (event.key) {
      case "ArrowRight":
      case "ArrowDown":
        nextIndex = selectedIndex === lastIndex ? 0 : selectedIndex + 1;
        break;
      case "ArrowLeft":
      case "ArrowUp":
        nextIndex = selectedIndex === 0 ? lastIndex : selectedIndex - 1;
        break;
      case "Home":
        nextIndex = 0;
        break;
      case "End":
        nextIndex = lastIndex;
        break;
      default:
        return;
    }
    event.preventDefault();
    selectAt(nextIndex);
  };

  return (
    <div
      className="theme-switcher"
      role="radiogroup"
      aria-label="主题"
      onKeyDown={handleKeyDown}
    >
      {THEME_OPTIONS.map((option, index) => {
        const Icon = option.icon;
        const checked = preference === option.value;
        return (
          <button
            key={option.value}
            ref={(element) => {
              optionRefs.current[index] = element;
            }}
            className="theme-switcher-option"
            type="button"
            role="radio"
            aria-checked={checked}
            aria-label={option.label}
            title={option.label}
            tabIndex={checked ? 0 : -1}
            onClick={() => {
              setPreference(option.value);
            }}
          >
            <Icon className="theme-switcher-icon" aria-hidden="true" size={16} />
            <span className="theme-switcher-label" aria-hidden="true">
              {option.shortLabel}
            </span>
          </button>
        );
      })}
    </div>
  );
}
