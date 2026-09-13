/**
 * Agent-notify opencode 插件：会话任务完成时调用 agent-notify.exe 推送微信消息。
 *
 * 形状：opencode 桌面端加载器要求的 V2 定义 `{ id, setup }`，零顶层 import，
 * 不依赖 node_modules 里版本不一致的 `@opencode-ai/plugin`。
 *
 * 触发：`session.execution.succeeded`（桌面端实测的任务完成事件）。
 * 摘要：取该会话最近消息里最后一条 assistant 文本，压成一行交给 CLI 排版。
 * 去重：同一会话按冷却时间只推一次，多 location 重复投递靠共享状态文件兜住。
 * 安全：失败全部吞掉，永远不影响 agent 运行。
 *
 * 结构：加载器要求零顶层 import，因此保持单文件、按段落组织：
 *   路径常量 → 通用小工具 → 引用回复收件箱/心跳 → 通知推送 → setup 入口。
 *
 * 路径：安装器会把安装目录里的绝对路径写进下面的 BAKED_BIN；仓库内直接运行时该值为空，
 * 依次回退到环境变量、%USERPROFILE%\bin\agent-notify.exe 和 PATH。
 *
 * 环境变量（改完要重启 opencode 桌面端才生效）：
 * - AGENT_NOTIFY_BIN：覆盖 agent-notify.exe 路径，优先级高于安装器写入的路径
 * - AGENT_NOTIFY_OPENCODE_MARKER_FILE：开关 marker，存在即停，默认
 *   %USERPROFILE%\.config\agent-notify\opencode.off
 * - AGENT_NOTIFY_COOLDOWN_MIN：同会话冷却分钟数，默认读配置文件，兜底 10
 * - AGENT_NOTIFY_DRYRUN=1：只渲染不发送（联调用）
 * - AGENT_NOTIFY_OFF=1：插件总开关，直接不拉起 CLI
 * - AGENT_NOTIFY_DEBUG=1：把判定与调用结果写进 %TEMP%\agent-notify\opencode-debug.log
 * - AGENT_NOTIFY_OPENCODE_REPLY_TIMEOUT_MS：引用回复提交超时毫秒数，默认 30000
 * - AGENT_NOTIFY_OPENCODE_FETCH_TIMEOUT_MS：会话标题/摘要读取超时毫秒数，默认 10000
 *
 * 勿扰时段与标题免打扰由 CLI 统一判定，插件不重复实现。
 */

const HOME_DIR =
  (typeof process !== "undefined" &&
    (process.env.USERPROFILE || process.env.HOME)) ||
  ""

const TMP_BASE =
  (typeof process !== "undefined" &&
    (process.env.TEMP ||
      process.env.TMP ||
      (process.env.USERPROFILE
        ? `${process.env.USERPROFILE}/AppData/Local/Temp`
        : ""))) ||
  ""

const TEMP_DIR =
  (typeof process !== "undefined" && process.env.AGENT_NOTIFY_TEMP_DIR) ||
  (TMP_BASE ? `${TMP_BASE}/agent-notify` : "agent-notify")

const CONFIG_FILE =
  (typeof process !== "undefined" && process.env.AGENT_NOTIFY_CONFIG_FILE) ||
  (HOME_DIR ? `${HOME_DIR}/.config/agent-notify/config.json` : "")

const MARKER_FILE =
  (typeof process !== "undefined" &&
    process.env.AGENT_NOTIFY_OPENCODE_MARKER_FILE) ||
  (HOME_DIR ? `${HOME_DIR}/.config/agent-notify/opencode.off` : "")

const STATE_FILE =
  (typeof process !== "undefined" && process.env.AGENT_NOTIFY_STATE_FILE) ||
  `${TEMP_DIR}/opencode-sent.json`

const DEBUG_LOG_FILE = `${TEMP_DIR}/opencode-debug.log`

const CONFIG_DIR =
  (typeof process !== "undefined" && process.env.AGENT_NOTIFY_CONFIG_DIR) ||
  (HOME_DIR ? `${HOME_DIR}/.config/agent-notify` : "")

const REPLY_DIR =
  (typeof process !== "undefined" && process.env.AGENT_NOTIFY_OPENCODE_REPLY_DIR) ||
  (CONFIG_DIR ? `${CONFIG_DIR}/opencode-reply-inbox` : "")

const REPLY_PENDING_DIR = `${REPLY_DIR}/pending`
const REPLY_PROCESSING_DIR = `${REPLY_DIR}/processing`
const REPLY_RESULT_DIR = `${REPLY_DIR}/results`
const REPLY_HEARTBEAT_DIR = `${REPLY_DIR}/heartbeats`
const MILLISECONDS_PER_SECOND = 1000
const MILLISECONDS_PER_MINUTE = 60 * MILLISECONDS_PER_SECOND
const REPLY_HEARTBEAT_MS = 5 * MILLISECONDS_PER_SECOND
const REPLY_HEARTBEAT_MAX_AGE_MS = 30 * MILLISECONDS_PER_SECOND
const REPLY_HEARTBEAT_FUTURE_SKEW_MS = 5 * MILLISECONDS_PER_SECOND
const REPLY_PROCESSING_STALE_MS = REPLY_HEARTBEAT_MAX_AGE_MS
const REPLY_ARTIFACT_TTL_MS = 10 * MILLISECONDS_PER_MINUTE
const REPLY_INSTANCE_RANDOM_LENGTH = 8
const NOTIFY_CHILD_TIMEOUT_MS = 25 * MILLISECONDS_PER_SECOND
const NOTIFY_CALLBACK_TIMEOUT_MS = 30 * MILLISECONDS_PER_SECOND
const DEFAULT_REPLY_PROMPT_TIMEOUT_MS = 30 * MILLISECONDS_PER_SECOND
const DEFAULT_SESSION_FETCH_TIMEOUT_MS = 10 * MILLISECONDS_PER_SECOND

const RAW_SUMMARY_CHARS = 0
const NOTIFY_MAX_CHARS = 0
const DEBUG_EXCERPT_CHARS = 200
const REPLY_ERROR_MAX_CHARS = 500
const SESSION_TITLE_MAX_CHARS = 0
const DEFAULT_SESSION_TITLE = "opencode会话"
const SENT_STATE_MAX_ENTRIES = 500
const DEFAULT_COOLDOWN_MIN = 10
const PRIVATE_FILE_MODE = 0o600
const PRIVATE_DIRECTORY_MODE = 0o700

// 安装器会把引号里的值替换为实际安装路径；仓库内副本保持空串，走下面的探测链。
const BAKED_BIN = ""

type FsModule = typeof import("node:fs")
type DebugLogger = (message: string) => void
type JsonRecord = Record<string, unknown>
type SentState = Record<string, number>

interface SessionApi {
  context(input: { sessionID: string }): Promise<unknown>
  get(input: { sessionID: string }): Promise<unknown>
  prompt?(input: PromptCurrentInput): Promise<unknown>
  promptAsync?(input: PromptAsyncInput): Promise<unknown>
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

interface PromptCurrentInput {
  sessionID: string
  text: string
  delivery: "steer" | "queue"
}

interface PromptAsyncLegacyInput {
  path: { id: string }
  body: {
    parts: Array<{ type: "text"; text: string }>
  }
  throwOnError: true
}

interface PromptAsyncDirectInput {
  sessionID: string
  parts: Array<{ type: "text"; text: string }>
  throwOnError: true
}

type PromptAsyncInput = PromptAsyncLegacyInput | PromptAsyncDirectInput


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
let cooldownMs: number | null = null
let replyPumpRunning = false

function envValue(name: string): string {
  return (typeof process !== "undefined" && process.env[name]) || ""
}

function limitText(text: string, maxChars: number): string {
  return maxChars > 0 ? text.slice(0, maxChars) : text
}

function existsInFs(path: string): boolean {
  try {
    return Boolean(fsMod && path && fsMod.existsSync(path))
  } catch {
    return false
  }
}

function resolveTarget(): string {
  const configured = envValue("AGENT_NOTIFY_BIN")
  if (configured) {
    return configured
  }
  // 安装器写入的绝对路径优先于 %USERPROFILE%\bin 默认值，自定义安装目录才不会失联。
  if (existsInFs(BAKED_BIN)) {
    return BAKED_BIN
  }
  const home = envValue("USERPROFILE")
  if (home) {
    const installed = `${home}\\bin\\agent-notify.exe`
    if (existsInFs(installed)) {
      return installed
    }
  }
  return BAKED_BIN || "agent-notify.exe"
}

async function fsAsync(): Promise<FsModule | null> {
  if (!fsMod) {
    try {
      fsMod = await import("node:fs")
      try {
        fsMod.mkdirSync(TEMP_DIR, { recursive: true })
      } catch {
        /* 目录建失败不影响主流程 */
      }
    } catch {
      fsMod = null
    }
  }
  return fsMod
}

function dbg(msg: string): void {
  try {
    if (debugLog) {
      debugLog(msg)
    }
  } catch {
    /* 日志失败不影响主流程 */
  }
}

// 冷却时间来源优先级：环境变量 > config.json > 默认值。
function cooldown(): number {
  if (cooldownMs !== null) {
    return cooldownMs
  }
  let minutes = Number(envValue("AGENT_NOTIFY_COOLDOWN_MIN")) || 0
  if (!minutes && CONFIG_FILE && fsMod) {
    try {
      const raw = fsMod.readFileSync(CONFIG_FILE, "utf8")
      const parsed: unknown = JSON.parse(raw)
      const value = Number(isRecord(parsed) ? parsed.cooldownMin : 0)
      if (value > 0) {
        minutes = value
      }
    } catch {
      /* 配置缺失或损坏时退回默认值 */
    }
  }
  if (!(minutes > 0)) {
    minutes = DEFAULT_COOLDOWN_MIN
  }
  cooldownMs = minutes * MILLISECONDS_PER_MINUTE
  return cooldownMs
}

function markerOff(): boolean {
  try {
    if (!MARKER_FILE || !fsMod) {
      return false
    }
    return fsMod.existsSync(MARKER_FILE)
  } catch {
    return false
  }
}

function readSent(): SentState {
  try {
    if (!fsMod) {
      return {}
    }
    const parsed: unknown = JSON.parse(fsMod.readFileSync(STATE_FILE, "utf8"))
    if (!isRecord(parsed)) {
      return {}
    }
    const state: SentState = {}
    for (const [key, value] of Object.entries(parsed)) {
      const timestamp = Number(value)
      if (Number.isFinite(timestamp)) {
        state[key] = timestamp
      }
    }
    return state
  } catch {
    return {}
  }
}

function writeSent(map: SentState): void {
  try {
    if (fsMod) {
      fsMod.writeFileSync(STATE_FILE, JSON.stringify(map))
    }
  } catch {
    /* 状态落盘失败不影响推送 */
  }
}

function isRecord(value: unknown): value is JsonRecord {
  return typeof value === "object" && value !== null
}

function errorMessage(error: unknown): string {
  return errorMessageInner(error, new Set<object>())
}

function errorMessageInner(error: unknown, seen: Set<object>): string {
  if (typeof error === "string") {
    return error.trim()
  }
  if (typeof error === "number" || typeof error === "boolean") {
    return String(error)
  }
  if (!isRecord(error)) {
    return error == null ? "" : String(error)
  }
  if (seen.has(error)) {
    return ""
  }
  seen.add(error)

  for (const key of ["message", "reason", "_tag"]) {
    const value = error[key]
    if (typeof value === "string" && value.trim()) {
      return value.trim()
    }
  }
  for (const key of ["cause", "error", "data"]) {
    const nested = errorMessageInner(error[key], seen)
    if (nested) {
      return nested
    }
  }
  const name = error.name
  if (typeof name === "string" && name.trim() && !isGenericErrorName(name)) {
    return name.trim()
  }
  for (const key of ["status", "statusCode", "code"]) {
    const value = error[key]
    if (typeof value === "number" || (typeof value === "string" && value.trim())) {
      return key === "code" ? String(value) : `HTTP ${String(value).trim()}`
    }
  }
  try {
    const encoded = JSON.stringify(error)
    if (encoded && encoded !== "{}") {
      return encoded
    }
  } catch {
    /* 非序列化错误回退到空串 */
  }
  return ""
}

function isGenericErrorName(value: string): boolean {
  const normalized = value.trim().toLowerCase()
  return normalized === "error" || normalized === "unknownerror"
}

function replyErrorDetail(error: unknown, sensitiveText = ""): string {
  let detail = errorMessage(error).trim() || "未知错误"
  if (sensitiveText) {
    for (const variant of sensitiveTextVariants(sensitiveText)) {
      detail = detail.split(variant).join("[REDACTED]")
    }
  }
  return detail.slice(0, REPLY_ERROR_MAX_CHARS)
}

function sensitiveTextVariants(value: string): string[] {
  const variants = [value]
  const encoded = JSON.stringify(value)
  if (encoded.length >= 2) {
    variants.push(encoded.slice(1, -1))
  }
  return variants.filter((variant) => variant.length > 0)
}

function writeReplyFileAtomic(path: string, value: unknown): void {
  if (!fsMod) {
    throw new Error("fs unavailable")
  }
  const temp = `${path}.tmp-${process.pid}`
  let descriptor: number | null = null
  try {
    descriptor = fsMod.openSync(temp, "w", PRIVATE_FILE_MODE)
    fsMod.writeFileSync(descriptor, JSON.stringify(value))
    fsMod.fsyncSync(descriptor)
    fsMod.closeSync(descriptor)
    descriptor = null
  } catch (writeError) {
    if (descriptor !== null) {
      try {
        fsMod.closeSync(descriptor)
      } catch {
        /* 描述符已关闭 */
      }
    }
    try {
      fsMod.unlinkSync(temp)
    } catch {
      /* 临时文件已不存在 */
    }
    throw writeError
  }
  try {
    fsMod.renameSync(temp, path)
  } catch (renameError) {
    try {
      fsMod.unlinkSync(temp)
    } catch {
      /* 临时文件已不存在 */
    }
    throw renameError
  }
}

function ensureReplyDirs(): void {
  if (!fsMod || !REPLY_DIR) {
    return
  }
  for (const dir of [
    REPLY_DIR,
    REPLY_PENDING_DIR,
    REPLY_PROCESSING_DIR,
    REPLY_RESULT_DIR,
    REPLY_HEARTBEAT_DIR,
  ]) {
    fsMod.mkdirSync(dir, { recursive: true, mode: PRIVATE_DIRECTORY_MODE })
    try {
      fsMod.chmodSync(dir, PRIVATE_DIRECTORY_MODE)
    } catch {
      /* Windows 或不支持 chmod 的文件系统保持原权限 */
    }
  }
}

type PromptBinding =
  | {
      send: (input: PromptCurrentInput) => Promise<unknown>
      session: SessionApi
      shape: "current"
    }
  | {
      send: (input: PromptAsyncInput) => Promise<unknown>
      session: SessionApi
      shape: "legacy" | "direct"
    }

function replyPrompt(ctx: PluginContext): PromptBinding | undefined {
  if (typeof ctx.session?.prompt === "function") {
    return {
      send: ctx.session.prompt,
      session: ctx.session,
      shape: "current",
    }
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

function newReplyInstanceID(): string {
  const random = Math.random()
    .toString(36)
    .slice(2, 2 + REPLY_INSTANCE_RANDOM_LENGTH)
  return `${process.pid}-${Date.now().toString(36)}-${random}`
}

function writeReplyHeartbeat(ctx: PluginContext, instanceID: string): void {
  try {
    if (!fsMod || !REPLY_DIR || !instanceID) {
      return
    }
    ensureReplyDirs()
    writeReplyFileAtomic(`${REPLY_HEARTBEAT_DIR}/${instanceID}.json`, {
      ready: Boolean(replyPrompt(ctx)),
      timestamp: new Date().toISOString(),
    })
  } catch (error) {
    dbg(`reply heartbeat fail: ${errorMessage(error)}`)
  }
}

function clearReplyHeartbeat(instanceID: string): void {
  try {
    if (fsMod && REPLY_HEARTBEAT_DIR && instanceID) {
      fsMod.unlinkSync(`${REPLY_HEARTBEAT_DIR}/${instanceID}.json`)
    }
  } catch (error) {
    dbg(`reply heartbeat clear fail: ${errorMessage(error)}`)
  }
}

function replyOwnerIsActive(owner: unknown): boolean {
  if (
    typeof owner !== "string" ||
    !/^[A-Za-z0-9-]+$/.test(owner) ||
    !fsMod ||
    !REPLY_HEARTBEAT_DIR
  ) {
    return false
  }
  try {
    const parsed: unknown = JSON.parse(
      fsMod.readFileSync(`${REPLY_HEARTBEAT_DIR}/${owner}.json`, "utf8"),
    )
    if (!isRecord(parsed) || typeof parsed.timestamp !== "string") {
      return false
    }
    if (parsed.ready !== true) {
      return false
    }
    const timestamp = Date.parse(parsed.timestamp)
    return (
      Number.isFinite(timestamp) &&
      timestamp <= Date.now() + REPLY_HEARTBEAT_FUTURE_SKEW_MS &&
      Date.now() - timestamp <= REPLY_HEARTBEAT_MAX_AGE_MS
    )
  } catch {
    return false
  }
}

function writeReplyResult(jobID: string, ok: boolean, error?: string): void {
  try {
    if (!fsMod || !jobID) {
      return
    }
    ensureReplyDirs()
    writeReplyFileAtomic(`${REPLY_RESULT_DIR}/${jobID}.json`, { ok, error: error || "" })
  } catch (resultError) {
    dbg(`reply result fail job=${jobID} err=${errorMessage(resultError)}`)
  }
}

function recoverStaleReplyJobs(): void {
  if (!fsMod || !REPLY_DIR) {
    return
  }
  let names: string[]
  try {
    names = fsMod.readdirSync(REPLY_PROCESSING_DIR).filter((name) => name.endsWith(".json"))
  } catch {
    return
  }
  const now = Date.now()
  for (const name of names) {
    const processingPath = `${REPLY_PROCESSING_DIR}/${name}`
    const resultPath = `${REPLY_RESULT_DIR}/${name}`
    try {
      if (fsMod.existsSync(resultPath)) {
        fsMod.unlinkSync(processingPath)
        continue
      }
      let parsed: unknown
      try {
        parsed = JSON.parse(fsMod.readFileSync(processingPath, "utf8"))
      } catch {
        parsed = undefined
      }
      const owner = isRecord(parsed) ? parsed.owner : undefined
      const expiresAt = isRecord(parsed) && typeof parsed.expiresAt === "string"
        ? Date.parse(parsed.expiresAt)
        : 0
      if (Number.isFinite(expiresAt) && expiresAt > 0 && expiresAt <= now) {
        const jobID = name.replace(/\.json$/, "")
        writeReplyResult(
          jobID,
          false,
          "引用回复任务已过期且未确认，未自动重试",
        )
        fsMod.unlinkSync(processingPath)
        dbg(`reply expired job=${name}`)
        continue
      }
      if (replyOwnerIsActive(owner)) {
        continue
      }
      const info = fsMod.statSync(processingPath)
      if (now - info.mtimeMs < REPLY_PROCESSING_STALE_MS) {
        continue
      }
      // A processing file is an ownership claim. Requeueing it after a
      // crash could duplicate a session prompt that already reached OpenCode.
      // Report an interrupt instead and leave no retry behind.
      const jobID = name.replace(/\.json$/, "")
      writeReplyResult(jobID, false, "引用回复处理中断，未自动重试以避免重复执行")
      fsMod.unlinkSync(processingPath)
      dbg(`reply abandoned stale job=${name}`)
    } catch (error) {
      dbg(`reply recovery fail file=${name} err=${errorMessage(error)}`)
    }
  }
}

// 所有对外调用只允许占用有限时间：单个悬空请求不能永久占住插件状态。
async function withTimeout<T>(promise: Promise<T>, timeoutMs: number, message: string): Promise<T> {
  let timer: ReturnType<typeof setTimeout> | undefined
  try {
    return await Promise.race([
      promise,
      new Promise<never>((_resolve, reject) => {
        timer = setTimeout(() => {
          reject(new Error(message))
        }, timeoutMs)
      }),
    ])
  } finally {
    if (timer !== undefined) {
      clearTimeout(timer)
    }
  }
}

// 会话 prompt 只负责投递：请求悬空时必须让回复泵继续走，超时按“状态未知”上报且不重试。
function replyPromptTimeoutMs(): number {
  const configured = Number(envValue("AGENT_NOTIFY_OPENCODE_REPLY_TIMEOUT_MS"))
  return Number.isFinite(configured) && configured > 0
    ? configured
    : DEFAULT_REPLY_PROMPT_TIMEOUT_MS
}

function sessionFetchTimeoutMs(): number {
  const configured = Number(envValue("AGENT_NOTIFY_OPENCODE_FETCH_TIMEOUT_MS"))
  return Number.isFinite(configured) && configured > 0
    ? configured
    : DEFAULT_SESSION_FETCH_TIMEOUT_MS
}

async function promptExistingSession(
  ctx: PluginContext,
  sessionID: string,
  text: string,
): Promise<void> {
  const binding = replyPrompt(ctx)
  if (!binding) {
    throw new Error("当前 OpenCode 插件不支持会话 prompt 投递")
  }
  let result: unknown
  if (binding.shape === "current") {
    result = await withTimeout(
      binding.send.call(binding.session, { sessionID, text, delivery: "steer" }),
      replyPromptTimeoutMs(),
      "引用回复提交超时，未自动重试以避免重复执行",
    )
  } else {
    const parts = [{ type: "text" as const, text }]
    const request: PromptAsyncInput = binding.shape === "legacy"
      ? {
          path: { id: sessionID },
          body: { parts },
          throwOnError: true,
        }
      : { sessionID, parts, throwOnError: true }
    result = await withTimeout(
      binding.send.call(binding.session, request),
      replyPromptTimeoutMs(),
      "引用回复提交超时，未自动重试以避免重复执行",
    )
  }
  if (isRecord(result) && result.error != null) {
    throw new Error(errorMessage(result.error))
  }
}

function readReplyJobExpiry(path: string): number {
  if (!fsMod) {
    return 0
  }
  try {
    const parsed: unknown = JSON.parse(fsMod.readFileSync(path, "utf8"))
    if (!isRecord(parsed) || typeof parsed.expiresAt !== "string") {
      return 0
    }
    const expiresAt = Date.parse(parsed.expiresAt)
    return Number.isFinite(expiresAt) ? expiresAt : 0
  } catch {
    return 0
  }
}

function cleanupStaleReplyArtifacts(): void {
  if (!fsMod || !REPLY_DIR) {
    return
  }
  for (const dir of [
    REPLY_DIR,
    REPLY_PENDING_DIR,
    REPLY_PROCESSING_DIR,
    REPLY_RESULT_DIR,
    REPLY_HEARTBEAT_DIR,
  ]) {
    let names: string[]
    try {
      names = fsMod.readdirSync(dir)
    } catch {
      continue
    }
    for (const name of names) {
      const temporary = name.includes(".tmp")
      const staleResult = dir === REPLY_RESULT_DIR && name.endsWith(".json")
      const expiringPending = dir === REPLY_PENDING_DIR && name.endsWith(".json")
      const heartbeatLease = dir === REPLY_HEARTBEAT_DIR && name.endsWith(".json")
      if (!temporary && !staleResult && !expiringPending && !heartbeatLease) {
        continue
      }
      const path = `${dir}/${name}`
      try {
        const now = Date.now()
        let stale: boolean
        if (expiringPending) {
          const expiresAt = readReplyJobExpiry(path)
          stale = expiresAt > 0
            ? expiresAt <= now
            : fsMod.statSync(path).mtimeMs < now - REPLY_ARTIFACT_TTL_MS
        } else {
          const ageLimit = heartbeatLease ? REPLY_HEARTBEAT_MAX_AGE_MS : REPLY_ARTIFACT_TTL_MS
          stale = fsMod.statSync(path).mtimeMs < now - ageLimit
        }
        if (stale) {
          fsMod.unlinkSync(path)
        }
      } catch (error) {
        dbg(`reply cleanup fail file=${name} err=${errorMessage(error)}`)
      }
    }
  }
}

async function processReplyJobs(ctx: PluginContext, instanceID: string): Promise<void> {
  if (replyPumpRunning || !fsMod || !REPLY_DIR) {
    return
  }
  if (!instanceID) {
    return
  }
  replyPumpRunning = true
  try {
    ensureReplyDirs()
    cleanupStaleReplyArtifacts()
    if (!replyPrompt(ctx)) {
      return
    }
    recoverStaleReplyJobs()
    const names = fsMod.readdirSync(REPLY_PENDING_DIR).filter((name) => name.endsWith(".json"))
    for (const name of names) {
      const pendingPath = `${REPLY_PENDING_DIR}/${name}`
      const processingPath = `${REPLY_PROCESSING_DIR}/${name}`
      const jobID = name.replace(/\.json$/, "")
      const resultPath = `${REPLY_RESULT_DIR}/${name}`
      let claimed = false
      try {
        // A durable result is the at-most-once record for this job ID. Drop a
        // duplicate pending file instead of invoking session prompt again.
        if (fsMod.existsSync(resultPath)) {
          try {
            fsMod.unlinkSync(pendingPath)
            dbg(`reply duplicate suppressed job=${name}`)
          } catch (error) {
            dbg(`reply duplicate cleanup fail file=${name} err=${errorMessage(error)}`)
          }
          continue
        }
        try {
          fsMod.renameSync(pendingPath, processingPath)
        } catch {
          continue
        }
        claimed = true
        try {
          const claimedAt = new Date()
          fsMod.utimesSync(processingPath, claimedAt, claimedAt)
        } catch {
          /* 文件系统不支持更新时间时仍按原 mtime 处理 */
        }
        const parsed: unknown = JSON.parse(fsMod.readFileSync(processingPath, "utf8"))
        if (!isRecord(parsed)) {
          writeReplyResult(jobID, false, "引用回复任务格式无效")
          continue
        }
        if (typeof parsed.id !== "string" || parsed.id.trim() !== jobID) {
          writeReplyResult(jobID, false, "引用回复任务 ID 不一致")
          continue
        }
        writeReplyFileAtomic(processingPath, { ...parsed, owner: instanceID })
        const sessionID = typeof parsed.sessionID === "string" ? parsed.sessionID.trim() : ""
        const text = typeof parsed.text === "string" ? parsed.text.trim() : ""
        const expiresAt = typeof parsed.expiresAt === "string" ? Date.parse(parsed.expiresAt) : 0
        if (!sessionID || !text) {
          writeReplyResult(jobID, false, "引用回复任务字段不完整")
          continue
        }
        if (!Number.isFinite(expiresAt) || expiresAt <= Date.now()) {
          writeReplyResult(jobID, false, "引用回复任务已过期")
          continue
        }
        try {
          await promptExistingSession(ctx, sessionID, text)
          writeReplyResult(jobID, true)
          dbg(`reply sent sid=${sessionID}`)
        } catch (error) {
          const detail = replyErrorDetail(error, text)
          writeReplyResult(jobID, false, detail)
          dbg(`reply failed sid=${sessionID} err=${detail}`)
        }
      } catch (error) {
        const detail = replyErrorDetail(error)
        writeReplyResult(jobID, false, "引用回复任务读取失败")
        dbg(`reply job fail file=${name} err=${detail}`)
      } finally {
        if (claimed) {
          try {
            fsMod.unlinkSync(processingPath)
          } catch {
            /* 已被插件或清理逻辑移除 */
          }
        }
      }
    }
  } catch (error) {
    dbg(`reply pump fail: ${errorMessage(error)}`)
  } finally {
    replyPumpRunning = false
  }
}

function spawnNotify(
  title: string,
  summary: string,
  sessionID: string,
): Promise<void> {
  const target = resolveTarget()
  const args = [
    "notify",
    "--agent",
    "opencode",
    "--title",
    title,
    "--summary",
    summary,
    "--session",
    sessionID,
    "--max-chars",
    String(NOTIFY_MAX_CHARS),
    "--no-stdin",
  ]
  if (envValue("AGENT_NOTIFY_DRYRUN") === "1") {
    args.push("--dry-run")
  }
  dbg(`spawn sid=${sessionID} target=${target} args=${args.length}`)

  return new Promise<void>((resolve) => {
    let settled = false
    const done = () => {
      if (!settled) {
        settled = true
        resolve()
      }
    }
    const timer = setTimeout(done, NOTIFY_CALLBACK_TIMEOUT_MS)
    import("node:child_process")
      .then(({ execFile }) => {
        const child = execFile(
          target,
          args,
          { timeout: NOTIFY_CHILD_TIMEOUT_MS, windowsHide: true },
          (error, stdout, stderr) => {
            clearTimeout(timer)
            dbg(
              `exit sid=${sessionID} err=${error ? errorMessage(error) : "none"} stderr=${String(stderr || "").slice(0, DEBUG_EXCERPT_CHARS)} out=${String(stdout || "").slice(0, DEBUG_EXCERPT_CHARS)}`,
            )
            done()
          },
        )
        // notify 收到显式 --summary 后不会再读 stdin，这里关掉管道防子进程挂起。
        try {
          child.stdin?.end()
        } catch {
          /* 忽略 */
        }
        child.on("error", (error) => {
          clearTimeout(timer)
          dbg(`error sid=${sessionID} ${errorMessage(error)}`)
          done()
        })
      })
      .catch(done)
  })
}

/** 取会话最后一条 assistant 文本，截断到上限；拿不到返回空串。 */
async function lastAssistantText(
  session: SessionApi,
  sessionID: string,
): Promise<string> {
  const res = await session.context({ sessionID })
  const data = Array.isArray(res)
    ? res
    : isRecord(res)
      ? res.data
      : undefined
  if (!Array.isArray(data)) {
    return ""
  }
  for (let i = data.length - 1; i >= 0; i--) {
    const item = data[i]
    if (!isRecord(item)) {
      continue
    }
    const infoRole = isRecord(item.info) ? item.info.role : undefined
    const role =
      typeof infoRole === "string"
        ? infoRole
        : typeof item.role === "string"
          ? item.role
          : item.type
    if (role !== "assistant") {
      continue
    }
    if (typeof item.text === "string") {
      const text = item.text.trim()
      if (text.length > 0) {
        return limitText(text, RAW_SUMMARY_CHARS)
      }
      continue
    }
    for (const bucket of [item.parts, item.content]) {
      if (!Array.isArray(bucket)) {
        continue
      }
      const texts: string[] = []
      for (const part of bucket) {
        if (
          isRecord(part) &&
          part.type === "text" &&
          typeof part.text === "string" &&
          part.text.trim().length > 0
        ) {
          texts.push(part.text)
        }
      }
      const text = texts.join("\n").trim()
      if (text.length > 0) {
        return limitText(text, RAW_SUMMARY_CHARS)
      }
    }
  }
  return ""
}

async function sessionTitle(
  session: SessionApi,
  sessionID: string,
): Promise<string> {
  const res = await session.get({ sessionID })
  let info: unknown
  if (!Array.isArray(res)) {
    info = isRecord(res) && isRecord(res.data) ? res.data : res
  }
  const title = isRecord(info) ? info.title : undefined
  return typeof title === "string" && title.trim().length > 0
    ? limitText(title.trim(), SESSION_TITLE_MAX_CHARS)
    : DEFAULT_SESSION_TITLE
}

const lastSent = new Map<string, number>()
const pending = new Set<string>()

async function handleTaskComplete(
  ctx: PluginContext,
  sessionID: string,
): Promise<void> {
  await fsAsync()
  if (envValue("AGENT_NOTIFY_OFF") === "1") {
    dbg("skip: OFF=1")
    return
  }
  if (markerOff()) {
    dbg(`skip: marker-off file=${MARKER_FILE}`)
    return
  }
  if (typeof sessionID !== "string" || sessionID.length === 0) {
    dbg("skip: bad sessionID")
    return
  }

  const now = Date.now()
  const wait = cooldown()
  if (now - (lastSent.get(sessionID) || 0) < wait) {
    dbg(`skip: cooldown sid=${sessionID}`)
    return
  }
  if (pending.has(sessionID)) {
    dbg(`skip: in-flight sid=${sessionID}`)
    return
  }
  const sent = readSent()
  if (now - (Number(sent[sessionID]) || 0) < wait) {
    lastSent.set(sessionID, now)
    dbg(`skip: file-cooldown sid=${sessionID}`)
    return
  }

  pending.add(sessionID)
  lastSent.set(sessionID, now)
  if (lastSent.size > SENT_STATE_MAX_ENTRIES) {
    lastSent.clear()
  }
  sent[sessionID] = now
  for (const key of Object.keys(sent)) {
    if (now - Number(sent[key]) > wait) {
      delete sent[key]
    }
  }
  writeSent(sent)

  try {
    let title = DEFAULT_SESSION_TITLE
    try {
      title = await withTimeout(
        sessionTitle(ctx.session, sessionID),
        sessionFetchTimeoutMs(),
        "读取会话标题超时",
      )
    } catch (error) {
      dbg(`title fail sid=${sessionID} err=${errorMessage(error)}`)
    }

    let summary = ""
    try {
      summary = await withTimeout(
        lastAssistantText(ctx.session, sessionID),
        sessionFetchTimeoutMs(),
        "读取会话摘要超时",
      )
    } catch (error) {
      dbg(`summary fail sid=${sessionID} err=${errorMessage(error)}`)
    }

    await spawnNotify(`【opencode】${title}`, summary, sessionID)
    dbg(`pushed sid=${sessionID}`)
  } finally {
    // 无论元数据读取或推送是否成功，都必须释放 in-flight 标记，避免该会话永久静默。
    pending.delete(sessionID)
  }
}

export default {
  id: "agent-notify",
  setup: async (ctx: PluginContext) => {
    await fsAsync()
    try {
      if (envValue("AGENT_NOTIFY_DEBUG") === "1") {
        const fs = await import("node:fs")
        debugLog = (msg: string) => {
          fs.appendFileSync(
            DEBUG_LOG_FILE,
            `${new Date().toISOString()} ${msg}\n`,
          )
        }
        dbg("setup ok")
      }
    } catch {
      debugLog = null
    }

    const replyInstanceID = newReplyInstanceID()
    writeReplyHeartbeat(ctx, replyInstanceID)
    void processReplyJobs(ctx, replyInstanceID)
    const replyTimer = setInterval(() => {
      writeReplyHeartbeat(ctx, replyInstanceID)
      void processReplyJobs(ctx, replyInstanceID)
    }, REPLY_HEARTBEAT_MS)

    const controller = new AbortController()
    const pump = (async () => {
      try {
        for await (const event of ctx.event.subscribe({
          signal: controller.signal,
        })) {
          try {
            if (!isRecord(event)) {
              continue
            }
            const type = event.type
            if (type !== "session.execution.succeeded") {
              continue
            }
            const props = event.properties || event.data
            const direct = event.sessionID
            const sessionID =
              (isRecord(props) && typeof props.sessionID === "string"
                ? props.sessionID
                : "") || (typeof direct === "string" ? direct : "")
            if (!sessionID) {
              continue
            }
            void handleTaskComplete(ctx, sessionID)
          } catch {
            /* 单个事件失败不影响后续 */
          }
        }
      } catch {
        /* 订阅结束（dispose）是正常情况 */
      }
    })()
    void pump

    return () => {
      clearInterval(replyTimer)
      controller.abort()
      clearReplyHeartbeat(replyInstanceID)
    }
  },
}
