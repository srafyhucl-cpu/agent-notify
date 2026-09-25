import { useId, type ReactNode } from "react";

/** 空态漏斗的单步说明。 */
export interface EmptyFunnelStep {
  /** 步骤标题 */
  title: string;
  /** 步骤补充说明（可选） */
  description?: string;
}

/** 渠道漏斗空态属性：锚点大标题 + 一句话说明 + 可选三步 + 主 CTA。 */
export interface EmptyFunnelProps {
  /** 锚点大标题（如「先连接渠道」） */
  title: string;
  /** 一句话说明：讲因果，文案由调用方传入 */
  description: string;
  /** 可选三步说明列表 */
  steps?: readonly EmptyFunnelStep[];
  /** 主 CTA（如「添加渠道账号」） */
  action?: ReactNode;
}

/**
 * 渠道漏斗空态：全 App 最重要的空态。
 * 面向"先连接渠道"场景——锚点标题 + 因果说明 + 三步 + 主 CTA；文案与 CTA 均由调用方传入，
 * 本组件只负责容器与层级，不发明交互。
 */
export function EmptyFunnel({
  title,
  description,
  steps,
  action,
}: EmptyFunnelProps) {
  const titleId = useId();

  return (
    <section className="pattern-empty-funnel" aria-labelledby={titleId}>
      <div className="pattern-empty-funnel-copy">
        <h2 className="pattern-empty-funnel-title" id={titleId}>
          {title}
        </h2>
        <p className="pattern-empty-funnel-description">{description}</p>
      </div>
      {steps && steps.length > 0 ? (
        <ol className="pattern-empty-funnel-steps">
          {steps.map((step, index) => (
            <li className="pattern-empty-funnel-step" key={index}>
              <span className="pattern-empty-funnel-step-title">
                {step.title}
              </span>
              {step.description ? (
                <span className="pattern-empty-funnel-step-description">
                  {step.description}
                </span>
              ) : null}
            </li>
          ))}
        </ol>
      ) : null}
      {action ? (
        <div className="pattern-empty-funnel-action">{action}</div>
      ) : null}
    </section>
  );
}
