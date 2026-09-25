import { PanelLeftClose, PanelLeftOpen } from "lucide-react";
import { NavLink } from "react-router-dom";

import logoUrl from "../assets/logo.png";
import { navigationItems } from "../app/navigation";
import { ThemeSwitcher } from "./ThemeSwitcher";

export interface AppNavProps {
  collapsed?: boolean;
  onToggleCollapse?: () => void;
}

export function AppNav({ collapsed = false, onToggleCollapse }: AppNavProps) {
  return (
    <nav
      className={`app-nav ${collapsed ? "app-nav--collapsed" : ""}`}
      aria-label="主导航"
    >
      <div className="app-nav-header" data-tauri-drag-region>
        <NavLink
          className="app-nav-brand"
          to="/overview"
          aria-label="AgentNotify 总览"
          title="AgentNotify"
        >
          <img
            src={logoUrl}
            alt="AgentNotify Logo"
            className="app-nav-brand-logo"
            aria-hidden="true"
          />
          {!collapsed && (
            <span className="app-nav-brand-label">AgentNotify</span>
          )}
        </NavLink>
      </div>

      <ul className="app-nav-list">
        {navigationItems.map((item) => {
          const Icon = item.icon;
          return (
            <li key={item.path}>
              <NavLink
                className="app-nav-link"
                to={item.path}
                aria-label={item.label}
                title={collapsed ? item.label : undefined}
              >
                <Icon className="app-nav-icon" aria-hidden="true" size={18} />
                {!collapsed && (
                  <span className="app-nav-label">{item.label}</span>
                )}
              </NavLink>
            </li>
          );
        })}
      </ul>

      <div className="app-nav-footer">
        <ThemeSwitcher collapsed={collapsed} />
        {onToggleCollapse && (
          <button
            type="button"
            className="app-nav-collapse-btn"
            onClick={onToggleCollapse}
            aria-label={collapsed ? "展开导航栏" : "收起导航栏"}
            title={collapsed ? "展开导航栏" : "收起导航栏"}
          >
            {collapsed ? (
              <PanelLeftOpen size={16} aria-hidden="true" />
            ) : (
              <>
                <PanelLeftClose size={16} aria-hidden="true" />
                <span className="app-nav-collapse-label">收起侧栏</span>
              </>
            )}
          </button>
        )}
      </div>
    </nav>
  );
}
