/**
 * Agent-notify OpenCode V2 插件。
 *
 * 单文件、零顶层 import，直接通过 agentnotify-ingress.exe 提交完成事件；
 * 监听当前 OpenCode 的 session.idle / session.error / session.execution.failed，
 * 兼容旧版 session.execution.succeeded；
 * 同时维护本地引用回复收件箱和插件心跳。所有失败都会被吞掉，不能阻断 OpenCode。
 */

const HOME_DIR =
  (typeof process !== "undefined" &&
    (process.env.USERPROFILE || process.env.HOME)) ||
  ""
const CONFIG_DIR =
  (typeof process !== "undefined" && process.env.AGENT_NOTIFY_CONFIG_DIR) ||
  (HOME_DIR ? `${HOME_DIR}/.config/agent-notify` : "")
const REPLY_DIR =
  (typeof process !== "undefined" &&
    process.env.AGENT_NOTIFY_OPENCODE_REPLY_DIR) ||
  (CONFIG_DIR ? `${CONFIG_DIR}/opencode-reply-inbox` : "")
const TEMP_DIR =
  (typeof process !== "undefined" &&
    (process.env.AGENT_NOTIFY_TEMP_DIR ||
      (process.env.TEMP ? `${process.env.TEMP}/agent-notify` : ""))) ||
  ""
const DEBUG_LOG_FILE = TEMP_DIR ? `${TEMP_DIR}/opencode-debug.log` : ""
const BAKED_INGRESS = ""

const PENDING_DIR = `${REPLY_DIR}/pending`
const PROCESSING_DIR = `${REPLY_DIR}/processing`
const RESULT_DIR = `${REPLY_DIR}/results`
const HEARTBEAT_DIR = `${REPLY_DIR}/heartbeats`

const MILLISECONDS_PER_SECOND = 1000
const HEARTBEAT_INTERVAL_MS = 5 * MILLISECONDS_PER_SECOND
const HEARTBEAT_MAX_AGE_MS = 30 * MILLISECONDS_PER_SECOND
const HEARTBEAT_FUTURE_SKEW_MS = 5 * MILLISECONDS_PER_SECOND
const JOB_TTL_MS = 10 * 60 * MILLISECONDS_PER_SECOND
const INGRESS_CHILD_TIMEOUT_MS = 25 * MILLISECONDS_PER_SECOND
const INGRESS_CALLBACK_TIMEOUT_MS = 30 * MILLISECONDS_PER_SECOND
const DEFAULT_PROMPT_TIMEOUT_MS = 30 * MILLISECONDS_PER_SECOND
const SESSION_FETCH_TIMEOUT_MS = 10 * MILLISECONDS_PER_SECOND
const ERROR_MAX_CHARS = 300
const PRIVATE_FILE_MODE = 0o600
const EVENT_SESSION_IDLE = "session.idle"
const EVENT_SESSION_ERROR = "session.error"
const EVENT_SESSION_EXECUTION_SUCCEEDED = "session.execution.succeeded"
const EVENT_SESSION_EXECUTION_FAILED = "session.execution.failed"
const FAILED_IDLE_SUPPRESSION_MS = 2 * MILLISECONDS_PER_SECOND

type FsModule = typeof import("node:fs")
type JsonRecord = Record<string, unknown>
type DebugLogger = (message: string) => void
type TerminalEventKind = "completed" | "failed"
type IngressSubmitter = (envelope: JsonRecord) => Promise<void>

interface AssistantMessage {
  id: string
  text: string
  completedAt: string
  error: string
}
type PromptBinding =
  | {
      send: (input: {
        sessionID: string
        text: string
        delivery: "steer"
      }) => Promise<unknown>
      session: SessionApi
    }
  | {
      send: (input: unknown) => Promise<unknown>
      session: SessionApi
      shape: "legacy" | "direct"
    }

interface SessionApi {
  context(input: { sessionID: string }): Promise<unknown>
  get(input: { sessionID: string }): Promise<unknown>
  prompt?(input: {
    sessionID: string
    text: string
    delivery: "steer"
  }): Promise<unknown>
  promptAsync?(input: unknown): Promise<unknown>
}

interface PluginContext {
  event: {
    subscribe(options: { signal: AbortSignal }): AsyncIterable<unknown>
  }
  session: SessionApi
  client?: {
    session?: SessionApi
  }
}

interface ReplyJob {
  id: string
  sessionID: string
  text: string
  createdAt: string
  expiresAt: string
  owner?: string
}

let fsMod: FsModule | null = null
let debugLog: DebugLogger | null = null
let replyPumpRunning = false

function isRecord(value: unknown): value is JsonRecord {
  return typeof value === "object" && value !== null
}

function envValue(name: string): string {
  return (typeof process !== "undefined" && process.env[name]) || ""
}

function errorMessage(error: unknown): string {
  if (typeof error === "string") {
    return error.trim()
  }
  if (isRecord(error)) {
    for (const key of ["message", "reason", "code"]) {
      const value = error[key]
      if (typeof value === "string" && value.trim()) {
        return value.trim()
      }
    }
  }
  return error == null ? "" : String(error).trim()
}

function stringField(value: unknown, key: string): string {
  if (!isRecord(value)) {
    return ""
  }
  const field = value[key]
  return typeof field === "string" ? field.trim() : ""
}

function eventProperties(event: unknown): JsonRecord {
  if (!isRecord(event)) {
    return {}
  }
  if (isRecord(event.properties)) {
    return event.properties
  }
  if (isRecord(event.data)) {
    return event.data
  }
  return event
}

function terminalEventType(event: unknown): TerminalEventKind | undefined {
  const type = stringField(event, "type")
  if (type === EVENT_SESSION_ERROR) {
    return "failed"
  }
  if (type === EVENT_SESSION_EXECUTION_FAILED) {
    return "failed"
  }
  if (type === EVENT_SESSION_IDLE || type === EVENT_SESSION_EXECUTION_SUCCEEDED) {
    return "completed"
  }
  return undefined
}

function eventSessionID(event: unknown): string {
  const properties = eventProperties(event)
  const fromProperties = stringField(properties, "sessionID")
  if (fromProperties) {
    return fromProperties
  }
  return stringField(event, "sessionID")
}

function sessionErrorMessage(error: unknown): string {
  if (!isRecord(error)) {
    return errorMessage(error)
  }
  const message = stringField(error, "message")
  if (message) {
    return message
  }
  if (isRecord(error.data)) {
    const nested = sessionErrorMessage(error.data)
    if (nested) {
      return nested
    }
  }
  return stringField(error, "name") || stringField(error, "type")
}

function eventError(event: unknown): unknown {
  return eventProperties(event).error
}

function dbg(message: string): void {
  try {
    debugLog?.(message)
  } catch {
    // 日志失败不影响 OpenCode。
  }
}

async function fsAsync(): Promise<FsModule | null> {
  if (fsMod) {
    return fsMod
  }
  try {
    fsMod = await import("node:fs")
  } catch {
    fsMod = null
  }
  return fsMod
}

function exists(path: string): boolean {
  try {
    return Boolean(fsMod && path && fsMod.existsSync(path))
  } catch {
    return false
  }
}

function resolveIngressTarget(): string {
  const configured = envValue("AGENT_NOTIFY_INGRESS_BIN")
  if (configured) {
    return configured
  }
  if (exists(BAKED_INGRESS)) {
    return BAKED_INGRESS
  }
  const installed = HOME_DIR ? `${HOME_DIR}\\bin\\agentnotify-ingress.exe` : ""
  if (exists(installed)) {
    return installed
  }
  return "agentnotify-ingress.exe"
}

function promptTimeoutMs(): number {
  const configured = Number(envValue("AGENT_NOTIFY_OPENCODE_REPLY_TIMEOUT_MS"))
  return Number.isFinite(configured) && configured > 0
    ? configured
    : DEFAULT_PROMPT_TIMEOUT_MS
}

function replyPrompt(ctx: PluginContext): PromptBinding | undefined {
  if (typeof ctx.session?.prompt === "function") {
    return { send: ctx.session.prompt, session: ctx.session }
  }
  const clientSession = ctx.client?.session
  if (typeof clientSession?.promptAsync === "function") {
    return {
      send: clientSession.promptAsync,
      session: clientSession,
      shape: "legacy",
    }
  }
  if (typeof ctx.session?.promptAsync === "function") {
    return {
      send: ctx.session.promptAsync,
      session: ctx.session,
      shape: "direct",
    }
  }
  return undefined
}

function newInstanceID(): string {
  const random = Math.random().toString(36).slice(2, 10)
  return `${process.pid}-${Date.now().toString(36)}-${random}`
}

function writeAtomic(path: string, value: unknown): void {
  if (!fsMod) {
    throw new Error("fs unavailable")
  }
  const temporary = `${path}.tmp-${process.pid}-${Math.random().toString(36).slice(2)}`
  let descriptor: number | null = null
  try {
    descriptor = fsMod.openSync(temporary, "w", PRIVATE_FILE_MODE)
    fsMod.writeFileSync(descriptor, JSON.stringify(value))
    fsMod.fsyncSync(descriptor)
    fsMod.closeSync(descriptor)
    descriptor = null
    fsMod.renameSync(temporary, path)
  } catch (error) {
    if (descriptor !== null) {
      try {
        fsMod.closeSync(descriptor)
      } catch {
        // descriptor 已关闭。
      }
    }
    try {
      fsMod.unlinkSync(temporary)
    } catch {
      // 临时文件不存在。
    }
    throw error
  }
}

function ensureReplyDirs(): void {
  if (!fsMod || !REPLY_DIR) {
    return
  }
  for (const directory of [
    REPLY_DIR,
    PENDING_DIR,
    PROCESSING_DIR,
    RESULT_DIR,
    HEARTBEAT_DIR,
  ]) {
    fsMod.mkdirSync(directory, { recursive: true, mode: 0o700 })
    try {
      fsMod.chmodSync(directory, 0o700)
    } catch {
      // Windows 或无 chmod 的文件系统保持平台默认权限。
    }
  }
}

function writeHeartbeat(ctx: PluginContext, instanceID: string): void {
  try {
    if (!fsMod || !HEARTBEAT_DIR || !instanceID) {
      return
    }
    ensureReplyDirs()
    writeAtomic(`${HEARTBEAT_DIR}/${instanceID}.json`, {
      ready: Boolean(replyPrompt(ctx)),
      timestamp: new Date().toISOString(),
    })
  } catch (error) {
    dbg(`heartbeat fail: ${errorMessage(error)}`)
  }
}

function clearHeartbeat(instanceID: string): void {
  try {
    if (fsMod && HEARTBEAT_DIR && instanceID) {
      fsMod.unlinkSync(`${HEARTBEAT_DIR}/${instanceID}.json`)
    }
  } catch {
    // dispose 清理失败不能影响 OpenCode。
  }
}

function withTimeout<T>(
  promise: Promise<T>,
  timeoutMs: number,
  message: string,
): Promise<T> {
  let timer: ReturnType<typeof setTimeout> | undefined
  return Promise.race([
    promise,
    new Promise<never>((_resolve, reject) => {
      timer = setTimeout(() => reject(new Error(message)), timeoutMs)
    }),
  ]).finally(() => {
    if (timer !== undefined) {
      clearTimeout(timer)
    }
  })
}

async function promptExistingSession(
  ctx: PluginContext,
  job: ReplyJob,
): Promise<void> {
  const binding = replyPrompt(ctx)
  if (!binding) {
    throw new Error("当前 OpenCode 版本不支持会话 prompt")
  }
  if ("shape" in binding) {
    const parts = [{ type: "text", text: job.text }]
    const request =
      binding.shape === "legacy"
        ? {
            path: { id: job.sessionID },
            body: { parts },
            throwOnError: true,
          }
        : { sessionID: job.sessionID, parts, throwOnError: true }
    await withTimeout(
      binding.send.call(binding.session, request),
      promptTimeoutMs(),
      "引用回复提交超时，未自动重试以避免重复执行",
    )
    return
  }
  await withTimeout(
    binding.send.call(binding.session, {
      sessionID: job.sessionID,
      text: job.text,
      delivery: "steer",
    }),
    promptTimeoutMs(),
    "引用回复提交超时，未自动重试以避免重复执行",
  )
}

function writeResult(jobID: string, ok: boolean, error = ""): void {
  try {
    if (!fsMod || !jobID) {
      return
    }
    ensureReplyDirs()
    writeAtomic(`${RESULT_DIR}/${jobID}.json`, {
      ok,
      error: error.slice(0, ERROR_MAX_CHARS),
    })
  } catch (error) {
    dbg(`result fail job=${jobID}: ${errorMessage(error)}`)
  }
}

function safeJobError(error: unknown, text: string): string {
  let detail = errorMessage(error) || "引用回复执行失败"
  for (const value of text ? [text, JSON.stringify(text).slice(1, -1)] : []) {
    if (value) {
      detail = detail.split(value).join("[REDACTED]")
    }
  }
  return detail.slice(0, ERROR_MAX_CHARS)
}

function recoverProcessingJobs(): void {
  if (!fsMod || !PROCESSING_DIR) {
    return
  }
  let names: string[] = []
  try {
    names = fsMod.readdirSync(PROCESSING_DIR).filter((name) => name.endsWith(".json"))
  } catch {
    return
  }
  const now = Date.now()
  for (const name of names) {
    const processingPath = `${PROCESSING_DIR}/${name}`
    if (exists(`${RESULT_DIR}/${name}`)) {
      try {
        fsMod.unlinkSync(processingPath)
      } catch {
        // 文件已被其他实例清理。
      }
      continue
    }
    try {
      const age = now - fsMod.statSync(processingPath).mtimeMs
      if (age < HEARTBEAT_MAX_AGE_MS) {
        continue
      }
      const jobID = name.replace(/\.json$/, "")
      writeResult(jobID, false, "引用回复处理中断，未自动重试以避免重复执行")
      fsMod.unlinkSync(processingPath)
    } catch (error) {
      dbg(`stale job fail file=${name}: ${errorMessage(error)}`)
    }
  }
}

async function processReplyJobs(
  ctx: PluginContext,
  instanceID: string,
): Promise<void> {
  if (replyPumpRunning || !fsMod || !REPLY_DIR || !instanceID) {
    return
  }
  replyPumpRunning = true
  try {
    ensureReplyDirs()
    if (!replyPrompt(ctx)) {
      return
    }
    recoverProcessingJobs()
    const names = fsMod
      .readdirSync(PENDING_DIR)
      .filter((name) => name.endsWith(".json"))
      .sort()

    for (const name of names) {
      const pendingPath = `${PENDING_DIR}/${name}`
      const processingPath = `${PROCESSING_DIR}/${name}`
      const jobID = name.replace(/\.json$/, "")
      if (exists(`${RESULT_DIR}/${name}`)) {
        try {
          fsMod.unlinkSync(pendingPath)
        } catch {
          // 重复 job 已被另一个实例处理。
        }
        continue
      }
      try {
        fsMod.renameSync(pendingPath, processingPath)
      } catch {
        continue
      }

      try {
        const parsed = JSON.parse(fsMod.readFileSync(processingPath, "utf8"))
        if (!isRecord(parsed) || parsed.id !== jobID) {
          writeResult(jobID, false, "引用回复任务格式无效")
          continue
        }
        const job = parsed as unknown as ReplyJob
        job.owner = instanceID
        writeAtomic(processingPath, job)
        const expiresAt = Date.parse(job.expiresAt)
        if (
          typeof job.sessionID !== "string" ||
          !job.sessionID.trim() ||
          typeof job.text !== "string" ||
          !job.text.trim()
        ) {
          writeResult(jobID, false, "引用回复任务字段不完整")
          continue
        }
        if (!Number.isFinite(expiresAt) || expiresAt <= Date.now()) {
          writeResult(jobID, false, "引用回复任务已过期")
          continue
        }
        try {
          await promptExistingSession(ctx, job)
          writeResult(jobID, true)
        } catch (error) {
          writeResult(jobID, false, safeJobError(error, job.text))
        }
      } catch (error) {
        writeResult(jobID, false, `引用回复任务读取失败：${safeJobError(error, "")}`)
      } finally {
        try {
          fsMod.unlinkSync(processingPath)
        } catch {
          // 处理中断时保留 processing，供后续恢复窗口写出未知结果。
        }
      }
    }
  } catch (error) {
    dbg(`reply pump fail: ${errorMessage(error)}`)
  } finally {
    replyPumpRunning = false
  }
}

// FNV-1a 32 位哈希：只用于事件幂等键，非加密用途。
const FNV_OFFSET_BASIS = 2166136261
const FNV_PRIME = 16777619

function hashText(value: string): string {
  let hash = FNV_OFFSET_BASIS
  for (let index = 0; index < value.length; index += 1) {
    hash ^= value.charCodeAt(index)
    hash = Math.imul(hash, FNV_PRIME)
  }
  return (hash >>> 0).toString(16).padStart(8, "0")
}

function eventIdentity(event: unknown, sessionID: string): string {
  if (isRecord(event)) {
    for (const key of ["id", "eventID", "eventId"]) {
      const value = event[key]
      if (typeof value === "string" && value.trim()) {
        return value.trim()
      }
    }
    const properties = event.properties
    if (isRecord(properties)) {
      for (const key of ["id", "eventID", "eventId"]) {
        const value = properties[key]
        if (typeof value === "string" && value.trim()) {
          return value.trim()
        }
      }
    }
    try {
      const encoded = JSON.stringify(event)
      if (encoded) {
        return `event-${hashText(encoded)}`
      }
    } catch {
      // 非序列化事件使用会话 ID 作为最后回退。
    }
  }
  return `session-${sessionID}`
}

function explicitEventIdentity(event: unknown): string {
  if (!isRecord(event)) {
    return ""
  }
  for (const key of ["id", "eventID", "eventId"]) {
    const value = event[key]
    if (typeof value === "string" && value.trim()) {
      return value.trim()
    }
  }
  const properties = eventProperties(event)
  for (const key of ["id", "eventID", "eventId"]) {
    const value = properties[key]
    if (typeof value === "string" && value.trim()) {
      return value.trim()
    }
  }
  return ""
}

function errorReference(error: unknown): string {
  for (const key of ["ref", "messageID", "messageId", "id"]) {
    const value = stringField(error, key)
    if (value) {
      return value
    }
  }
  return ""
}

function terminalIdentity(
  sessionID: string,
  event: unknown,
  message: AssistantMessage | undefined,
  failure: string,
): string {
  if (message?.id) {
    return `message:${message.id}`
  }
  const eventID = explicitEventIdentity(event)
  if (eventID) {
    return eventID
  }
  const reference = errorReference(eventError(event))
  if (reference) {
    return `message:${reference}`
  }
  if (failure) {
    return `failure:${hashText(failure)}`
  }
  const completedAt = message?.completedAt || ""
  const fingerprint = `${sessionID}\u0000${completedAt}\u0000${message?.text || ""}`
  return `content:${hashText(fingerprint)}`
}

function completionEnvelope(
  sessionID: string,
  title: string,
  body: string,
  event: unknown,
  identity = "",
): JsonRecord {
  return {
    protocolVersion: 1,
    kind: "agent.event",
    requestId: crypto.randomUUID(),
    agentId: "opencode",
    payload: {
      eventType: "session.completed",
      idempotencyKey: `opencode:${sessionID}:${identity.trim() || eventIdentity(event, sessionID)}`,
      occurredAt: new Date().toISOString(),
      sessionId: sessionID,
      title: `【opencode】${title || "会话"}`,
      body,
      metadata: {},
    },
  }
}

function submitIngress(envelope: JsonRecord): Promise<void> {
  return new Promise<void>((resolve) => {
    let settled = false
    const done = () => {
      if (!settled) {
        settled = true
        resolve()
      }
    }
    const timer = setTimeout(done, INGRESS_CALLBACK_TIMEOUT_MS)
    import("node:child_process")
      .then(({ execFile }) => {
        const child = execFile(
          resolveIngressTarget(),
          [],
          { timeout: INGRESS_CHILD_TIMEOUT_MS, windowsHide: true },
          (error) => {
            clearTimeout(timer)
            if (error) {
              dbg(`ingress exit: ${errorMessage(error)}`)
            }
            done()
          },
        )
        child.on("error", (error) => {
          clearTimeout(timer)
          dbg(`ingress error: ${errorMessage(error)}`)
          done()
        })
        try {
          child.stdin?.end(JSON.stringify(envelope))
        } catch (error) {
          dbg(`ingress stdin: ${errorMessage(error)}`)
          done()
        }
      })
      .catch((error) => {
        clearTimeout(timer)
        dbg(`ingress import: ${errorMessage(error)}`)
        done()
      })
  })
}

async function sessionTitle(session: SessionApi, sessionID: string): Promise<string> {
  const response = await withTimeout(
    session.get({ sessionID }),
    SESSION_FETCH_TIMEOUT_MS,
    "读取会话标题超时",
  )
  const data =
    isRecord(response) && isRecord(response.data) ? response.data : response
  const title = isRecord(data) ? data.title : undefined
  return typeof title === "string" && title.trim() ? title.trim() : "会话"
}

const lastTerminalBySession = new Map<string, string>()
const failedAtBySession = new Map<string, number>()
const terminalQueues = new Map<string, Promise<void>>()

function assistantText(item: JsonRecord): string {
  if (typeof item.text === "string" && item.text.trim()) {
    return item.text.trim()
  }
  for (const bucket of [item.parts, item.content]) {
    if (!Array.isArray(bucket)) {
      continue
    }
    const text = bucket
      .filter(
        (part) =>
          isRecord(part) &&
          part.type === "text" &&
          typeof part.text === "string" &&
          part.text.trim(),
      )
      .map((part) => String((part as JsonRecord).text))
      .join("\n")
      .trim()
    if (text) {
      return text
    }
  }
  return ""
}

async function lastAssistantMessage(
  session: SessionApi,
  sessionID: string,
): Promise<AssistantMessage | undefined> {
  const response = await withTimeout(
    session.context({ sessionID }),
    SESSION_FETCH_TIMEOUT_MS,
    "读取会话摘要超时",
  )
  const data = Array.isArray(response)
    ? response
    : isRecord(response)
      ? response.data
      : undefined
  if (!Array.isArray(data)) {
    return undefined
  }
  for (let index = data.length - 1; index >= 0; index -= 1) {
    const item = data[index]
    if (!isRecord(item)) {
      continue
    }
    const info = isRecord(item.info) ? item.info : item
    const role =
      typeof info.role === "string"
        ? info.role
        : typeof item.role === "string"
          ? item.role
          : item.type
    if (role !== "assistant") {
      continue
    }
    const time = isRecord(info.time) ? info.time : undefined
    const completed = time?.completed
    return {
      id:
        stringField(info, "id") ||
        stringField(info, "messageID") ||
        stringField(item, "id") ||
        stringField(item, "messageID"),
      text: assistantText(item),
      completedAt:
        typeof completed === "number" || typeof completed === "string"
          ? String(completed)
          : "",
      error: sessionErrorMessage(info.error),
    }
  }
  return undefined
}

function failureBody(error: string): string {
  const detail = error.trim()
  return detail
    ? `任务执行失败：${detail}`
    : "任务执行失败：OpenCode 未返回具体错误信息"
}

async function dispatchTerminalEvent(
  ctx: PluginContext,
  sessionID: string,
  event: unknown,
  submit: IngressSubmitter = submitIngress,
  now = Date.now(),
): Promise<void> {
  const kind = terminalEventType(event)
  if (!kind || typeof sessionID !== "string" || !sessionID.trim()) {
    return
  }
  try {
    const [title, message] = await Promise.all([
      sessionTitle(ctx.session, sessionID).catch(() => "会话"),
      lastAssistantMessage(ctx.session, sessionID).catch(() => undefined),
    ])

    if (kind === "completed") {
      const failedAt = failedAtBySession.get(sessionID)
      if (failedAt !== undefined) {
        failedAtBySession.delete(sessionID)
        if (now - failedAt <= FAILED_IDLE_SUPPRESSION_MS) {
          dbg(`skip idle after failure sid=${sessionID}`)
          return
        }
      }
    }

    const failure =
      kind === "failed"
        ? sessionErrorMessage(eventError(event)) || message?.error || ""
        : message?.error || ""
    const body = failure ? failureBody(failure) : message?.text.trim() || ""
    if (!body) {
      dbg(`skip terminal without body sid=${sessionID}`)
      return
    }

    const identity = terminalIdentity(sessionID, event, message, failure)
    if (lastTerminalBySession.get(sessionID) === identity) {
      dbg(`skip duplicate terminal sid=${sessionID} key=${identity}`)
      return
    }

    const displayTitle = failure ? `${title}（任务失败）` : title
    await submit(
      completionEnvelope(sessionID, displayTitle, body, event, identity),
    )
    lastTerminalBySession.set(sessionID, identity)
    if (failure) {
      failedAtBySession.set(sessionID, now)
    }
    if (lastTerminalBySession.size > 256) {
      lastTerminalBySession.clear()
    }
    dbg(
      `terminal submitted kind=${failure ? "failed" : "completed"} sid=${sessionID} key=${identity}`,
    )
  } catch (error) {
    dbg(`terminal fail sid=${sessionID}: ${errorMessage(error)}`)
  }
}

function enqueueTerminalEvent(
  ctx: PluginContext,
  sessionID: string,
  event: unknown,
): Promise<void> {
  const previous = terminalQueues.get(sessionID) ?? Promise.resolve()
  let current: Promise<void>
  current = previous
    .catch(() => undefined)
    .then(() => dispatchTerminalEvent(ctx, sessionID, event))
    .finally(() => {
      if (terminalQueues.get(sessionID) === current) {
        terminalQueues.delete(sessionID)
      }
    })
  terminalQueues.set(sessionID, current)
  return current
}

function resetTerminalStateForTests(): void {
  lastTerminalBySession.clear()
  failedAtBySession.clear()
  terminalQueues.clear()
}

async function dispatchCompletion(
  ctx: PluginContext,
  sessionID: string,
  event: unknown,
): Promise<void> {
  await dispatchTerminalEvent(ctx, sessionID, event)
}

const __test = {
  dispatchTerminalEvent,
  dispatchCompletion,
  processReplyJobs,
  completionEnvelope,
  eventIdentity,
  terminalEventType,
  eventSessionID,
  resetTerminalStateForTests,
  replyPrompt,
}

export { __test }
export default {
  id: "agent-notify",
  setup: async (ctx: PluginContext) => {
    await fsAsync()
    if (envValue("AGENT_NOTIFY_DEBUG") === "1" && fsMod && DEBUG_LOG_FILE) {
      try {
        fsMod.mkdirSync(TEMP_DIR, { recursive: true })
        debugLog = (message) =>
          fsMod?.appendFileSync(
            DEBUG_LOG_FILE,
            `${new Date().toISOString()} ${message}\n`,
          )
      } catch {
        debugLog = null
      }
    }
    ensureReplyDirs()

    const instanceID = newInstanceID()
    writeHeartbeat(ctx, instanceID)
    void processReplyJobs(ctx, instanceID)
    const heartbeatTimer = setInterval(() => {
      writeHeartbeat(ctx, instanceID)
      void processReplyJobs(ctx, instanceID)
    }, HEARTBEAT_INTERVAL_MS)

    const controller = new AbortController()
    void (async () => {
      try {
        for await (const event of ctx.event.subscribe({
          signal: controller.signal,
        })) {
          const kind = terminalEventType(event)
          const sessionID = eventSessionID(event)
          if (!kind || !sessionID) {
            continue
          }
          dbg(`terminal event type=${stringField(event, "type")} sid=${sessionID}`)
          void enqueueTerminalEvent(ctx, sessionID, event)
        }
      } catch {
        // 订阅结束是 dispose 的正常路径。
      }
    })()

    return () => {
      clearInterval(heartbeatTimer)
      controller.abort()
      clearHeartbeat(instanceID)
    }
  },
}
