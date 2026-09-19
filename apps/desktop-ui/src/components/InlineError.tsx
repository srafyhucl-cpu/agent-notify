import { CircleAlert } from "lucide-react";
import type { ReactNode } from "react";

export interface InlineErrorProps {
  title: string;
  message: string;
  action?: ReactNode;
}

export function InlineError({ title, message, action }: InlineErrorProps) {
  return (
    <div className="inline-error" role="alert">
      <CircleAlert className="inline-error-icon" aria-hidden="true" size={18} />
      <div className="inline-error-copy">
        <strong className="inline-error-title">{title}</strong>
        <span className="inline-error-message">{message}</span>
      </div>
      {action ? <div className="inline-error-action">{action}</div> : null}
    </div>
  );
}
