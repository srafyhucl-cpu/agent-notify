import { useId, type ReactNode } from "react";

/** 分组卡属性：标题行（标题 + 可选计数 + 可选右上操作）+ 可选说明 + 内容区。 */
export interface SectionCardProps {
  /** 分组标题（h2），卡片标题行的唯一语法 */
  title: string;
  /** 标题右侧计数（如条目数） */
  count?: number;
  /** 右上操作区（按钮等） */
  action?: ReactNode;
  /** 标题下的一句话说明 */
  description?: string;
  /** 卡片内容 */
  children: ReactNode;
  /** 根元素附加类名（仅用于外层布局） */
  className?: string;
}

/**
 * 分组卡：卡片标题行的唯一语法——标题 + 计数 + 右上操作都进规定位置，各页不得自创。
 * 表面用 --surface-1 层（两主题自动换肤）；以 aria-labelledby 关联 h2，成为有名字的 region。
 */
export function SectionCard({
  title,
  count,
  action,
  description,
  children,
  className,
}: SectionCardProps) {
  const titleId = useId();
  const rootClassName = className
    ? `pattern-section-card ${className}`
    : "pattern-section-card";

  return (
    <section className={rootClassName} aria-labelledby={titleId}>
      <header className="pattern-section-card-header">
        <div className="pattern-section-card-heading">
          <h2 className="pattern-section-card-title" id={titleId}>
            {title}
          </h2>
          {typeof count === "number" ? (
            <span className="pattern-section-card-count">{count}</span>
          ) : null}
        </div>
        {action ? (
          <div className="pattern-section-card-action">{action}</div>
        ) : null}
      </header>
      {description ? (
        <p className="pattern-section-card-description">{description}</p>
      ) : null}
      <div className="pattern-section-card-body">{children}</div>
    </section>
  );
}
