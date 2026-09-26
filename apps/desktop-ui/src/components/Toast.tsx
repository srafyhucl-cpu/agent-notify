import { CircleCheck } from "lucide-react";

export interface ToastProps {
  message: string;
  /** 语义色调；默认 success，用于「已发送 / 已保存」这类结果确认。 */
  tone?: "success" | "info";
}

/**
 * 浮层提示：只做结果确认，不占用版面（固定贴窗口底部中央，
 * 不会挤掉内容，也不遮挡顶部状态栏与窗口按钮）。
 *
 * 自动消失由调用方控制（见 ChannelsPage 的 effect），这样各场景可以自己定停留时长；
 * 组件本身只负责渲染与无障碍播报（role=status 由屏幕阅读器礼貌播报）。
 */
export function Toast({ message, tone = "success" }: ToastProps) {
  return (
    <div className={`toast toast--${tone}`} role="status">
      <CircleCheck className="toast-icon" aria-hidden="true" size={16} />
      <span className="toast-message">{message}</span>
    </div>
  );
}
