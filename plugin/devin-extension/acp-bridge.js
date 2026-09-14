"use strict"

// Devin 桌面端为每个窗口常驻一个 `devin.exe acp` 子进程，ACP 会话的正文既可以从
// 聊天面板提交，也可以直接以 NDJSON 写入该子进程的 stdin。聊天面板通道受会话
// 运行态和 UI 焦点影响，会退回新对话或 Ask 模式；stdin 通道直接按 sessionId
// 定向，不依赖 Devin CLI 登录，也不会改动当前对话。

const ACP_ARGUMENT = "acp"
const AGENT_TYPE_FLAG = "--agent-type"
const DEVIN_EXECUTABLE_PATTERN = /(^|[\\/])devin(?:\.exe)?$/i
const DEFAULT_WRITE_TIMEOUT_MS = 5000
const REQUEST_ID_BASE = 9000000000

const ACP_ERROR_CODES = {
  CHANNEL_MISSING: "acp_channel_missing",
  CHANNEL_AMBIGUOUS: "acp_channel_ambiguous",
  WRITE_FAILED: "acp_write_failed",
}

const ACP_CHANNEL_MISSING_MESSAGE =
  "未找到 Devin 桌面端 ACP 通道，请重启 Devin 后重试；回复不会改投到新会话"
const ACP_CHANNEL_AMBIGUOUS_MESSAGE =
  "检测到多个 Devin ACP 通道，无法确定回复目标，请重启 Devin 后重试；回复不会改投到新会话"
const ACP_WRITE_FAILED_MESSAGE =
  "Devin 桌面端 ACP 通道写入失败，请重启 Devin 后重试；回复不会改投到新会话"

let requestCounter = 0

function nextRequestID() {
  requestCounter += 1
  return REQUEST_ID_BASE + requestCounter
}

// acpError 标记 ACP 通道自身的失败，供扩展层映射成微信可读的错误码。
function acpError(code, message, causeDetail) {
  const error = new Error(message)
  error.acpCode = code
  if (causeDetail !== undefined) {
    error.causeDetail = causeDetail
  }
  return error
}

function detailOf(error) {
  if (error instanceof Error && error.message) {
    return error.message
  }
  if (typeof error === "string" && error) {
    return error
  }
  try {
    return JSON.stringify(error)
  } catch {
    return String(error)
  }
}

// isDevinACPHandle 只认桌面端自己拉起的 Devin ACP 子进程；摘要等内部 agent
// 进程带 --agent-type，必须排除，避免把回复写进错误的会话。
function isDevinACPHandle(handle) {
  if (!handle || !handle.constructor || handle.constructor.name !== "ChildProcess") {
    return false
  }
  const spawnargs = handle.spawnargs
  if (!Array.isArray(spawnargs)) {
    return false
  }
  if (!spawnargs.some((argument) => DEVIN_EXECUTABLE_PATTERN.test(argument))) {
    return false
  }
  if (!spawnargs.includes(ACP_ARGUMENT)) {
    return false
  }
  return !spawnargs.includes(AGENT_TYPE_FLAG)
}

// findACPHandle 要求唯一候选：多个候选时无法确定目标会话属于哪个 ACP 进程，
// 宁可明确失败也不猜测。
function findACPHandle(handles) {
  const candidates = (Array.isArray(handles) ? handles : []).filter(isDevinACPHandle)
  if (candidates.length === 0) {
    throw acpError(ACP_ERROR_CODES.CHANNEL_MISSING, ACP_CHANNEL_MISSING_MESSAGE)
  }
  if (candidates.length > 1) {
    throw acpError(ACP_ERROR_CODES.CHANNEL_AMBIGUOUS, ACP_CHANNEL_AMBIGUOUS_MESSAGE)
  }
  return candidates[0]
}

// buildPromptLine 生成 ACP 的 session/prompt NDJSON；每行一个完整请求，末尾换行。
function buildPromptLine({ id, sessionId, text }) {
  if (typeof sessionId !== "string" || sessionId.length === 0) {
    throw acpError(
      ACP_ERROR_CODES.WRITE_FAILED,
      "Devin 回复任务缺少会话标识，请重新引用原通知后再试",
    )
  }
  if (typeof text !== "string" || text.length === 0) {
    throw acpError(
      ACP_ERROR_CODES.WRITE_FAILED,
      "Devin 回复内容为空，请重新引用原通知后再试",
    )
  }
  const payload = {
    jsonrpc: "2.0",
    id,
    method: "session/prompt",
    params: {
      sessionId,
      prompt: [{ type: "text", text }],
    },
  }
  return `${JSON.stringify(payload)}\n`
}

// isWritableStdin 校验 ACP 子进程仍可接收输入。
function isWritableStdin(stdin) {
  return Boolean(
    stdin &&
      typeof stdin.write === "function" &&
      stdin.destroyed !== true &&
      stdin.writable !== false &&
      stdin.writableEnded !== true,
  )
}

// writePromptLine 以回调确认字节已交给管道；EPIPE 等错误明确失败，不静默吞掉。
function writePromptLine(child, line, { timeoutMs = DEFAULT_WRITE_TIMEOUT_MS } = {}) {
  return new Promise((resolve, reject) => {
    const stdin = child ? child.stdin : undefined
    if (!isWritableStdin(stdin)) {
      reject(
        acpError(
          ACP_ERROR_CODES.WRITE_FAILED,
          ACP_WRITE_FAILED_MESSAGE,
          "ACP 子进程 stdin 不可写",
        ),
      )
      return
    }

    let settled = false
    let timer = null
    const finish = (error) => {
      if (settled) {
        return
      }
      settled = true
      if (timer !== null) {
        clearTimeout(timer)
      }
      stdin.removeListener("error", onError)
      if (error) {
        reject(error)
      } else {
        resolve(Buffer.byteLength(line, "utf8"))
      }
    }
    const onError = (error) => {
      finish(
        acpError(ACP_ERROR_CODES.WRITE_FAILED, ACP_WRITE_FAILED_MESSAGE, detailOf(error)),
      )
    }

    stdin.once("error", onError)
    if (timeoutMs > 0) {
      timer = setTimeout(() => {
        finish(
          acpError(
            ACP_ERROR_CODES.WRITE_FAILED,
            ACP_WRITE_FAILED_MESSAGE,
            `写入等待超过 ${timeoutMs} 毫秒`,
          ),
        )
      }, timeoutMs)
    }

    try {
      stdin.write(line, "utf8", (error) => {
        if (error) {
          onError(error)
          return
        }
        finish(null)
      })
    } catch (error) {
      onError(error)
    }
  })
}

// createACPStdioBridge 把“找进程 → 组请求 → 写管道”封成一个通道；依赖以参数
// 注入，便于单测覆盖查找和写入失败。
function createACPStdioBridge({
  getHandles,
  createRequestId = nextRequestID,
  writeTimeoutMs = DEFAULT_WRITE_TIMEOUT_MS,
  log = () => {},
} = {}) {
  function findHandle() {
    if (typeof getHandles !== "function") {
      throw acpError(
        ACP_ERROR_CODES.CHANNEL_MISSING,
        ACP_CHANNEL_MISSING_MESSAGE,
        "当前环境不支持读取进程句柄",
      )
    }
    let handles = []
    try {
      handles = getHandles()
    } catch (error) {
      throw acpError(ACP_ERROR_CODES.CHANNEL_MISSING, ACP_CHANNEL_MISSING_MESSAGE, detailOf(error))
    }
    return findACPHandle(handles)
  }

  function isAvailable() {
    try {
      findHandle()
      return true
    } catch {
      return false
    }
  }

  async function send({ sessionId, text }) {
    const child = findHandle()
    const id = createRequestId()
    const line = buildPromptLine({ id, sessionId, text })
    const bytes = await writePromptLine(child, line, { timeoutMs: writeTimeoutMs })
    log(`acp prompt written id=${id} session=${sessionId} bytes=${bytes}`)
    return { id, bytes }
  }

  return { isAvailable, send }
}

module.exports = {
  ACP_ERROR_CODES,
  ACP_CHANNEL_MISSING_MESSAGE,
  ACP_CHANNEL_AMBIGUOUS_MESSAGE,
  ACP_WRITE_FAILED_MESSAGE,
  DEFAULT_WRITE_TIMEOUT_MS,
  buildPromptLine,
  createACPStdioBridge,
  findACPHandle,
  isDevinACPHandle,
  writePromptLine,
}
