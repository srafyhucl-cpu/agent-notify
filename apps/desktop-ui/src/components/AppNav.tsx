import { NavLink } from "react-router-dom";

import { navigationItems } from "../app/navigation";

export function AppNav() {
  return (
    <nav className="app-nav" aria-label="主导航">
      <NavLink
        className="app-nav-brand"
        to="/overview"
        aria-label="AgentNotify 总览"
        title="AgentNotify"
      >
        <span className="app-nav-brand-mark" aria-hidden="true">
          A
        </span>
        <span className="app-nav-brand-label">AgentNotify</span>
      </NavLink>

      <ul className="app-nav-list">
        {navigationItems.map((item) => {
          const Icon = item.icon;
          return (
            <li key={item.path}>
              <NavLink
                className="app-nav-link"
                to={item.path}
                aria-label={item.label}
                title={item.label}
              >
                <Icon className="app-nav-icon" aria-hidden="true" size={18} />
                <span className="app-nav-label">{item.label}</span>
              </NavLink>
            </li>
          );
        })}
      </ul>
    </nav>
  );
}
