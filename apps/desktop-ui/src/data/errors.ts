import type {
  BusinessCommand,
  CommandError,
  DeliveryDto,
} from "../bridge/types";

import { isCommandError } from "./queryClient";

export interface UserErrorAction {
  label: string;
  command: BusinessCommand;
  payload: unknown;
}

export interface UserError {
  title: string;
  message: string;
  action?: UserErrorAction;
  diagnosticId?: string;
}

export interface UserErrorContext {
  delivery?: {
    id: string;
    state: DeliveryDto["state"];
  };
}

const AUTHENTICATION_ERROR = {
  title: "登录状态已失效",
  message: "登录状态已失效，请在渠道页面重新登录后重试。",
} as const;

const CREDENTIAL_ERROR = {
  title: "登录凭据不可用",
  message: "登录凭据不可用，请在渠道页面重新登录对应账号。",
} as const;

const DATABASE_ERROR = {
  title: "数据库操作未完成",
  message: "数据库操作未完成，请先备份数据，然后查看诊断页面了解详情。",
} as const;

const UNKNOWN_DELIVERY_ERROR = {
  title: "投递结果未确认",
  message: "投递结果未确认，请先检查原渠道是否已收到消息；确认前不会自动重发。",
} as const;

const UNKNOWN_ERROR = {
  title: "操作未完成",
  message: "操作未完成，请稍后重试。",
} as const;

function normalizedCode(error: CommandError): string {
  return error.code.trim().toLowerCase();
}

function isAuthenticationError(code: string): boolean {
  return (
    code === "401" ||
    code.includes("http_401") ||
    code.includes("unauthorized") ||
    code.includes("authentication")
  );
}

function isCredentialError(code: string): boolean {
  return code.includes("credential") || code.includes("secret");
}

function isDatabaseError(code: string): boolean {
  return (
    code.includes("database") ||
    code.includes("sqlite") ||
    code.includes("migration") ||
    code === "db_locked"
  );
}

function isUnknownDeliveryError(code: string): boolean {
  return (
    code.includes("delivery") &&
    (code.includes("unknown") || code.includes("unconfirmed"))
  );
}

export function toUserError(
  error: unknown,
  context: UserErrorContext = {},
): UserError {
  if (context.delivery?.state === "Unknown") {
    return { ...UNKNOWN_DELIVERY_ERROR };
  }

  if (!isCommandError(error)) {
    return { ...UNKNOWN_ERROR };
  }

  const code = normalizedCode(error);
  const diagnosticId = error.diagnosticId ?? undefined;

  if (isUnknownDeliveryError(code)) {
    return { ...UNKNOWN_DELIVERY_ERROR, diagnosticId };
  }

  if (isAuthenticationError(code)) {
    return { ...AUTHENTICATION_ERROR, diagnosticId };
  }

  if (isCredentialError(code)) {
    return { ...CREDENTIAL_ERROR, diagnosticId };
  }

  if (isDatabaseError(code)) {
    return {
      ...DATABASE_ERROR,
      diagnosticId,
      action: {
        label: "查看诊断",
        command: "get_diagnostics",
        payload: {},
      },
    };
  }

  return {
    title: "操作未完成",
    message: error.message.trim() || UNKNOWN_ERROR.message,
    diagnosticId,
  };
}
