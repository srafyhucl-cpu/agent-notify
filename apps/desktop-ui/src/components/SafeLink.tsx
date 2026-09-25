import type { AnchorHTMLAttributes, ReactNode } from "react";
import { Link, useInRouterContext, type LinkProps } from "react-router-dom";

export interface SafeLinkProps
  extends Omit<AnchorHTMLAttributes<HTMLAnchorElement>, "href"> {
  to: string;
  children?: ReactNode;
}

/**
 * 具有环境自适应能力的路由链接组件。
 * 在 Router 上下文内消费 React Router Link 实现客户端无刷新跳转；
 * 在独立单测或无 Router 上下文时优雅降级为原生 <a>，防止 useContext 为空抛错。
 */
export function SafeLink({ to, children, ...rest }: SafeLinkProps) {
  const inRouter = useInRouterContext();
  if (inRouter) {
    return (
      <Link to={to} {...(rest as Omit<LinkProps, "to">)}>
        {children}
      </Link>
    );
  }
  return (
    <a href={to} {...rest}>
      {children}
    </a>
  );
}
