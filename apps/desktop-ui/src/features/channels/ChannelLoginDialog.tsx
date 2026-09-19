import {
  CircleAlert,
  LoaderCircle,
  QrCode,
  RefreshCw,
  X,
} from "lucide-react";
import {
  useEffect,
  useRef,
  useState,
  type MouseEvent,
} from "react";

import type { ChannelDto } from "../../bridge/types";
import { InlineError } from "../../components/InlineError";
import { toUserError } from "../../data/errors";
import type { ActiveChannelLogin } from "./useChannelLogin";

const LOGIN_STATE_LABELS = {
  Idle: "尚未开始登录",
  Preparing: "正在准备登录",
  QrReady: "二维码已就绪",
  WaitingScan: "等待扫码",
  NeedVerifyCode: "需要配对码",
  WaitingFirstInbound: "等待首条入站消息",
  Paired: "配对已完成",
  Expired: "二维码已过期",
  Blocked: "登录被阻止",
  Failed: "登录失败",
} as const;

function isDisplayableQrPayload(payload: string | null): boolean {
  return payload?.startsWith("data:image/") ?? false;
}

export interface ChannelLoginDialogProps {
  open: boolean;
  channel: ChannelDto | null;
  active: ActiveChannelLogin | null;
  isPreparing: boolean;
  isSubmitting: boolean;
  actionError: unknown;
  onBegin: (channelId: string) => void;
  onRefresh: () => void;
  onSubmitCode: (code: string) => void;
  onClose: () => void;
}

export function ChannelLoginDialog({
  open,
  channel,
  active,
  isPreparing,
  isSubmitting,
  actionError,
  onBegin,
  onRefresh,
  onSubmitCode,
  onClose,
}: ChannelLoginDialogProps) {
  const [code, setCode] = useState("");
  const closeButtonRef = useRef<HTMLButtonElement>(null);
  const codeInputRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    if (!open) {
      setCode("");
      return;
    }

    const handleKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        onClose();
      }
    };
    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, [onClose, open]);

  useEffect(() => {
    if (!open) {
      return;
    }

    if (active?.session.state === "NeedVerifyCode") {
      codeInputRef.current?.focus();
    } else {
      closeButtonRef.current?.focus();
    }
  }, [active?.session.state, open]);

  if (!open || !channel) {
    return null;
  }

  const session = active?.session ?? null;
  const state = isPreparing ? "Preparing" : (session?.state ?? "Idle");
  const stateLabel = LOGIN_STATE_LABELS[state];
  const userError = actionError ? toUserError(actionError) : null;
  const qrPayload = session?.qrPayload ?? null;
  const canShowQr =
    state === "QrReady" || state === "WaitingScan"
      ? isDisplayableQrPayload(qrPayload)
      : false;

  const handleOverlayMouseDown = (event: MouseEvent<HTMLDivElement>) => {
    if (event.target === event.currentTarget) {
      onClose();
    }
  };

  const submitCode = () => {
    const submittedCode = code;
    if (submittedCode.trim() === "") {
      return;
    }
    setCode("");
    onSubmitCode(submittedCode);
  };

  return (
    <div
      className="dialog-overlay"
      onMouseDown={handleOverlayMouseDown}
      role="presentation"
    >
      <section
        className="channel-login-dialog"
        role="dialog"
        aria-modal="true"
        aria-labelledby="channel-login-title"
        aria-busy={isPreparing || isSubmitting}
      >
        <header className="dialog-header">
          <div>
            <h2 id="channel-login-title">登录 {channel.displayName}</h2>
            <p className="dialog-subtitle">{stateLabel}</p>
          </div>
          <button
            ref={closeButtonRef}
            className="icon-button"
            type="button"
            aria-label="关闭登录窗口"
            onClick={onClose}
          >
            <X aria-hidden="true" size={18} />
          </button>
        </header>

        <div className="channel-login-body">
          {userError ? (
            <InlineError title={userError.title} message={userError.message} />
          ) : null}

          {state === "Idle" ? (
            <div className="login-state-panel">
              <QrCode aria-hidden="true" size={28} />
              <p>尚未建立登录会话，可以从这里开始扫码登录。</p>
              <button
                className="button"
                type="button"
                onClick={() => onBegin(channel.id)}
              >
                开始登录
              </button>
            </div>
          ) : null}

          {state === "Preparing" ? (
            <div className="login-state-panel" role="status">
              <LoaderCircle
                className="spin-icon"
                aria-hidden="true"
                size={28}
              />
              <p>正在准备登录会话，请稍候。</p>
            </div>
          ) : null}

          {canShowQr ? (
            <div className="qr-panel">
              <img
                className="channel-login-qr"
                src={qrPayload ?? ""}
                alt="渠道登录二维码"
              />
              <div>
                <strong>{state === "WaitingScan" ? "请扫码确认登录" : "请扫码登录"}</strong>
                <p>{session?.message ?? "二维码仅保存在内存中，关闭窗口不会取消登录任务。"}</p>
              </div>
            </div>
          ) : null}

          {(state === "QrReady" || state === "WaitingScan") &&
          !canShowQr ? (
            <div className="login-state-panel login-state-panel--error" role="alert">
              <CircleAlert aria-hidden="true" size={28} />
              <p>宿主未提供可显示的二维码图片，请刷新登录会话后重试。</p>
              <button className="button button-secondary" type="button" onClick={onRefresh}>
                <RefreshCw aria-hidden="true" size={15} />
                刷新二维码
              </button>
            </div>
          ) : null}

          {state === "NeedVerifyCode" ? (
            <form
              className="verify-code-form"
              onSubmit={(event) => {
                event.preventDefault();
                submitCode();
              }}
            >
              <label htmlFor="channel-login-code">配对码</label>
              <p>{session?.message ?? "请输入渠道客户端显示的配对码。"}</p>
              <input
                ref={codeInputRef}
                id="channel-login-code"
                type="password"
                inputMode="numeric"
                autoComplete="one-time-code"
                value={code}
                disabled={isSubmitting}
                onChange={(event) => setCode(event.currentTarget.value)}
              />
              <button
                className="button"
                type="submit"
                disabled={isSubmitting || code.trim() === ""}
              >
                {isSubmitting ? "正在提交" : "提交配对码"}
              </button>
            </form>
          ) : null}

          {state === "WaitingFirstInbound" ? (
            <div className="login-state-panel" role="status">
              <LoaderCircle
                className="spin-icon"
                aria-hidden="true"
                size={28}
              />
              <strong>等待首条入站消息</strong>
              <p>{session?.message ?? "登录已确认，收到首条消息后才能进行回复路由。"}</p>
            </div>
          ) : null}

          {state === "Paired" ? (
            <div className="login-state-panel login-state-panel--success" role="status">
              <strong>登录成功</strong>
              <p>{session?.message ?? "账号已经可以接收与发送消息。"}</p>
              <button className="button" type="button" onClick={onClose}>
                完成
              </button>
            </div>
          ) : null}

          {state === "Expired" ? (
            <div className="login-state-panel login-state-panel--error" role="alert">
              <CircleAlert aria-hidden="true" size={28} />
              <p>{session?.message ?? "二维码已过期，请刷新后重新扫码。"}</p>
              <button className="button" type="button" onClick={onRefresh}>
                <RefreshCw aria-hidden="true" size={15} />
                刷新二维码
              </button>
            </div>
          ) : null}

          {state === "Blocked" || state === "Failed" ? (
            <div className="login-state-panel login-state-panel--error" role="alert">
              <CircleAlert aria-hidden="true" size={28} />
              <strong>{state === "Blocked" ? "登录被阻止" : "登录失败"}</strong>
              <p>
                {session?.error?.message ??
                  session?.message ??
                  (state === "Blocked"
                    ? "当前登录请求被渠道阻止，请稍后重试。"
                    : "登录未完成，请重新开始。")}
              </p>
              <button className="button" type="button" onClick={onRefresh}>
                <RefreshCw aria-hidden="true" size={15} />
                重试登录
              </button>
            </div>
          ) : null}
        </div>
      </section>
    </div>
  );
}
