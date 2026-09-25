import type { ReactNode } from "react";

/** 页头属性：标题 + 一句话摘要 + 右侧主操作区。 */
export interface PageHeaderProps {
  /** 页面标题（h1），使用展示字阶 --font-display */
  title: string;
  /** 一句话摘要：讲清页面目的或因果；可不传 */
  summary?: string;
  /** 右侧主操作区（主按钮等）；可不传 */
  actions?: ReactNode;
}

/**
 * 页头范式：全站页面标题的唯一语法。
 * 标题固定 h1（每页仅一个），摘要降噪，主操作靠右；只消费语义 token，两主题自动成立。
 */
export function PageHeader({ title, actions }: PageHeaderProps) {
  return (
    <header className={`pattern-page-header ${!actions ? "pattern-page-header--empty" : ""}`}>
      <h1 className="sr-only">{title}</h1>
      {actions ? (
        <div className="pattern-page-header-actions">{actions}</div>
      ) : null}
    </header>
  );
}
