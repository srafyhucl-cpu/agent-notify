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
  create?(input: { title?: string }): Promise<unknown>
  context(input: { sessionID: string }): Promise<unknown>
  get(input: { sessionID: string }): Promise<unknown>
  prompt?(input: {
    sessionID: string
    text: string
    delivery: "steer"
  }): Promise<unknown>
  promptAsync?(input: unknown): Promise<unknown>
}

/** 权限 evaluate hook 事件（OpenCode v2；effect 可变，见设计文档 §6）。 */
interface PermissionEvaluation {
  sessionID: string
  agent?: string
  action: string
  resources: readonly string[]
  metadata?: Record<string, unknown>
  source?: unknown
  effect: "allow" | "ask" | "deny"
  message?: string
}

interface PermissionHookRegistration {
  dispose?(): void | Promise<void>
}

interface PermissionApi {
  hook?(
    name: "evaluate",
    handler: (event: PermissionEvaluation) => void | Promise<void>,
  ): unknown
}

interface PluginContext {
  event: {
    subscribe(options: { signal: AbortSignal }): AsyncIterable<unknown>
  }
  session: SessionApi
  client?: {
    session?: SessionApi
  }
  /** OpenCode v2 权限 API；旧版宿主可能缺失（缺失时权限无人值守降级）。 */
  permission?: PermissionApi
}

interface ReplyJob {
  id: string
  sessionID: string
  text: string
  createdAt: string
  expiresAt: string
  owner?: string
  /** true 时以 sessionID 作为新会话首个 prompt 发起开工；缺省按 false（续聊既有会话）。 */
  open?: boolean
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

/** 实例 id → 创建时间（instanceID = `${pid}-${Date.now().toString(36)}-${random}`）。 */
function instanceCreatedAt(instanceID: string): number {
  const encoded = instanceID.split("-")[1] ?? ""
  const parsed = encoded ? Number.parseInt(encoded, 36) : Number.NaN
  return Number.isFinite(parsed) ? parsed : 0
}

/**
 * 是否已被更新的活实例取代：**插件热重载会留下不退出旧实例**（旧实例心跳仍在刷新、
 * 事件订阅可能仍在提交），旧实例检测到新实例后应自行退休，避免多实例重复上报。
 * 判断依据：心跳目录里存在比本实例更新（或同刻 id 更大）且仍新鲜的心跳。
 */
function supersededByNewerInstance(instanceID: string): boolean {
  if (!fsMod || !HEARTBEAT_DIR) {
    return false
  }
  let names: string[] = []
  try {
    names = fsMod.readdirSync(HEARTBEAT_DIR)
  } catch {
    return false
  }
  const now = Date.now()
  const mine = instanceCreatedAt(instanceID)
  for (const name of names) {
    if (!name.endsWith(".json")) {
      continue
    }
    const other = name.slice(0, -".json".length)
    if (other === instanceID) {
      continue
    }
    try {
      const age = now - fsMod.statSync(`${HEARTBEAT_DIR}/${name}`).mtimeMs
      if (age > HEARTBEAT_MAX_AGE_MS) {
        continue
      }
    } catch {
      continue
    }
    const theirs = instanceCreatedAt(other)
    if (theirs > mine || (theirs === mine && other > instanceID)) {
      return true
    }
  }
  return false
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

// ---------------------------------------------------------------------------
// 编排会话映射（open=true 必须创建真实 OpenCode 会话，见 opencode.ai/v2 插件文档）
// ---------------------------------------------------------------------------
// 编排层用合成会话 id（`task-<task_id>-step-<n>`）标识每步会话；OpenCode 只认自己
// 的真实会话 id（`ses...`，服务端校验）。因此：
// - 合成 id 无映射（每步首个 prompt：step 1 是 open=true，后续 step 是 open=false）：
//   `ctx.session.create` 建真实会话，登记 合成→真实 映射后 prompt 真实 id；
// - 合成 id 有映射 / 普通会话 id：按映射换真实 id 直通 prompt（微信引用回复不受影响）；
// - 完成事件回传：真实 id 换回合成 id，桌面端才能把汇报归到对应任务/步骤。

/** 编排合成会话 id 前缀（与 Rust 侧 ORC_DISPATCH_SESSION_PREFIX 一致）。 */
const ORC_SESSION_PREFIX = "task-"
/** 会话映射文件（与回复收件箱同目录；原子写）。 */
const SESSION_MAP_FILE = REPLY_DIR ? `${REPLY_DIR}/session-map.json` : ""
/** 映射容量上限：超出后丢弃最旧条目（保序 Map）。 */
const SESSION_MAP_LIMIT = 2048

let sessionMapCache: Map<string, string> | null = null
/** 上次从磁盘强制刷新映射的时间（节流：避免每个未命中事件都读盘）。 */
let sessionMapRefreshedAt = 0
let sessionMapRefreshMinIntervalMs = 3 * MILLISECONDS_PER_SECOND

/**
 * 读取会话映射（进程内缓存）。`refresh=true` 时强制从磁盘重读——
 * 多实例/热重载期间其他实例写入的映射需要及时可见，避免映射缺失误报原始 id。
 */
function loadSessionMap(refresh = false): Map<string, string> {
  if (sessionMapCache && !refresh) {
    return sessionMapCache
  }
  sessionMapRefreshedAt = Date.now()
  const map = new Map<string, string>()
  try {
    if (fsMod && SESSION_MAP_FILE && fsMod.existsSync(SESSION_MAP_FILE)) {
      const parsed = JSON.parse(fsMod.readFileSync(SESSION_MAP_FILE, "utf8"))
      if (isRecord(parsed)) {
        for (const [key, value] of Object.entries(parsed)) {
          if (typeof value === "string" && value) {
            map.set(key, value)
          }
        }
      }
    }
  } catch (error) {
    dbg(`session map read fail: ${errorMessage(error)}`)
  }
  sessionMapCache = map
  return map
}

/** 未命中时按节流刷新磁盘映射（多实例写入的映射对本实例可见）。 */
function refreshSessionMapThrottled(): void {
  if (Date.now() - sessionMapRefreshedAt < sessionMapRefreshMinIntervalMs) {
    return
  }
  loadSessionMap(true)
}

/** 测试专用：调整刷新节流窗口（0 = 每次都刷新）。 */
function setSessionMapRefreshIntervalForTests(intervalMs: number): void {
  sessionMapRefreshMinIntervalMs = Number.isFinite(intervalMs) && intervalMs >= 0
    ? intervalMs
    : 0
}

function persistSessionMap(): void {
  try {
    if (!fsMod || !SESSION_MAP_FILE || !sessionMapCache) {
      return
    }
    const record: JsonRecord = {}
    for (const [key, value] of [...sessionMapCache.entries()].slice(
      -SESSION_MAP_LIMIT,
    )) {
      record[key] = value
    }
    writeAtomic(SESSION_MAP_FILE, record)
  } catch (error) {
    dbg(`session map write fail: ${errorMessage(error)}`)
  }
}

function isOrchestrationSessionID(sessionID: string): boolean {
  return sessionID.startsWith(ORC_SESSION_PREFIX)
}

/** 真实会话 id → 合成会话 id（无映射原样返回，普通会话不受影响）。 */
function mappedSessionID(sessionID: string): string {
  let map = loadSessionMap()
  for (const [synthetic, real] of map) {
    if (real === sessionID) {
      return synthetic
    }
  }
  // 未命中：可能映射由其他（热重载前）实例写入，按节流从磁盘刷新一次再找。
  refreshSessionMapThrottled()
  map = loadSessionMap()
  for (const [synthetic, real] of map) {
    if (real === sessionID) {
      return synthetic
    }
  }
  return sessionID
}

function createdSessionID(response: unknown): string {
  const data =
    isRecord(response) && isRecord(response.data) ? response.data : response
  const id = isRecord(data) ? data.id : undefined
  return typeof id === "string" && id.startsWith("ses") ? id.trim() : ""
}

async function createRealSession(
  ctx: PluginContext,
  syntheticID: string,
): Promise<string> {
  const create = ctx.session?.create
  if (typeof create !== "function") {
    throw new Error("当前 OpenCode 版本不支持创建会话，请升级 OpenCode 后重试")
  }
  const created = await withTimeout(
    create.call(ctx.session, { title: `【集群】${syntheticID}` }),
    SESSION_FETCH_TIMEOUT_MS,
    "创建会话超时",
  )
  const real = createdSessionID(created)
  if (!real) {
    throw new Error("创建会话未返回有效会话 id（ses...）")
  }
  return real
}

/**
 * 解析 prompt 目标会话 id：合成 id 换成真实 id；无映射时创建并登记。
 *
 * 每 (task, step) 一个新会话：step 1 以 open=true 首次派活，后续 step 以
 * open=false 首次派活（新合成 id 首次出现）——两种都是"该步会话尚未建立"，
 * 统一走创建。open 仅表示编排侧的语义提示，不改变本函数行为。
 * 非合成 id 直通（普通会话 / 微信引用回复保持原语义）。
 */
async function resolvePromptSessionID(
  ctx: PluginContext,
  job: ReplyJob,
): Promise<string> {
  if (!isOrchestrationSessionID(job.sessionID)) {
    return job.sessionID
  }
  let map = loadSessionMap()
  let existing = map.get(job.sessionID)
  if (!existing) {
    // 未命中：可能映射由其他（热重载前）实例写入，按节流刷新一次再找。
    refreshSessionMapThrottled()
    map = loadSessionMap()
    existing = map.get(job.sessionID)
  }
  if (existing) {
    return existing
  }
  const real = await createRealSession(ctx, job.sessionID)
  map.set(job.sessionID, real)
  persistSessionMap()
  return real
}

async function promptSessionText(
  ctx: PluginContext,
  sessionID: string,
  text: string,
  timeoutMessage: string,
): Promise<void> {
  const binding = replyPrompt(ctx)
  if (!binding) {
    throw new Error("当前 OpenCode 版本不支持会话 prompt")
  }
  if ("shape" in binding) {
    const parts = [{ type: "text", text }]
    const request =
      binding.shape === "legacy"
        ? {
            path: { id: sessionID },
            body: { parts },
            throwOnError: true,
          }
        : { sessionID, parts, throwOnError: true }
    await withTimeout(
      binding.send.call(binding.session, request),
      promptTimeoutMs(),
      timeoutMessage,
    )
    return
  }
  await withTimeout(
    binding.send.call(binding.session, {
      sessionID,
      text,
      delivery: "steer",
    }),
    promptTimeoutMs(),
    timeoutMessage,
  )
}

/** 处理一条回复任务：合成 id 先解析为真实会话 id（open=true 时创建），再 prompt。 */
async function promptJob(ctx: PluginContext, job: ReplyJob): Promise<void> {
  const sessionID = await resolvePromptSessionID(ctx, job)
  await promptSessionText(
    ctx,
    sessionID,
    job.text,
    job.open === true
      ? "发起新会话超时，未自动重试以避免重复执行"
      : "引用回复提交超时，未自动重试以避免重复执行",
  )
}

// ---------------------------------------------------------------------------
// 编排会话权限无人值守（设计文档 §6）
// ---------------------------------------------------------------------------
// 编排流程无人应答，权限弹窗会把步骤卡死。OpenCode v2 的 evaluate hook 在
// 允许/询问决策后、执行或弹窗前回调：仅对映射表内的编排真实会话把 ask 改成
// allow；显式 deny 不会触发 hook，普通会话/未知会话保持原行为。

/** evaluate 回调：编排会话的 ask 放行为 allow，其余原样。 */
function evaluatePermission(event: PermissionEvaluation): void {
  if (!isRecord(event) || event.effect !== "ask") {
    return
  }
  const sessionID = typeof event.sessionID === "string" ? event.sessionID : ""
  if (!sessionID || mappedSessionID(sessionID) === sessionID) {
    return
  }
  event.effect = "allow"
  dbg(
    `permission auto-allow sid=${sessionID} action=${stringField(event, "action")}`,
  )
}

/** 注册权限 evaluate hook；宿主不支持或注册失败时降级返回 undefined，不影响其它功能。 */
async function registerPermissionAutopilot(
  ctx: PluginContext,
): Promise<(() => void) | undefined> {
  const permission = ctx.permission
  const hook = permission?.hook
  if (typeof hook !== "function" || !permission) {
    dbg("permission hook unavailable: 编排会话权限弹窗无法自动放行")
    return undefined
  }
  try {
    const registration = await hook.call(permission, "evaluate", (event) => {
      evaluatePermission(event)
    })
    if (!isRecord(registration) || typeof registration.dispose !== "function") {
      return undefined
    }
    const dispose = registration.dispose as () => void | Promise<void>
    return () => {
      try {
        void Promise.resolve(dispose.call(registration)).catch((error) => {
          dbg(`permission hook dispose fail: ${errorMessage(error)}`)
        })
      } catch (error) {
        dbg(`permission hook dispose fail: ${errorMessage(error)}`)
      }
    }
  } catch (error) {
    dbg(`permission hook register fail: ${errorMessage(error)}`)
    return undefined
  }
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
          await promptJob(ctx, job)
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
      completionEnvelope(
        mappedSessionID(sessionID),
        displayTitle,
        body,
        event,
        identity,
      ),
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

function resetSessionMapForTests(): void {
  sessionMapCache = new Map()
  sessionMapRefreshedAt = 0
  try {
    if (fsMod && SESSION_MAP_FILE && fsMod.existsSync(SESSION_MAP_FILE)) {
      fsMod.unlinkSync(SESSION_MAP_FILE)
    }
  } catch (error) {
    dbg(`session map reset fail: ${errorMessage(error)}`)
  }
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
  promptJob,
  resolvePromptSessionID,
  mappedSessionID,
  instanceCreatedAt,
  supersededByNewerInstance,
  evaluatePermission,
  completionEnvelope,
  eventIdentity,
  terminalEventType,
  eventSessionID,
  resetTerminalStateForTests,
  resetSessionMapForTests,
  setSessionMapRefreshIntervalForTests,
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
    const disposePermissionHook = await registerPermissionAutopilot(ctx)

    const instanceID = newInstanceID()
    writeHeartbeat(ctx, instanceID)

    const controller = new AbortController()
    let heartbeatTimer: ReturnType<typeof setInterval> | undefined
    const dispose = () => {
      if (heartbeatTimer !== undefined) {
        clearInterval(heartbeatTimer)
        heartbeatTimer = undefined
      }
      controller.abort()
      clearHeartbeat(instanceID)
      disposePermissionHook?.()
    }

    void processReplyJobs(ctx, instanceID)
    heartbeatTimer = setInterval(() => {
      // 热重载可能留下旧实例（其心跳仍在刷新）：检测到更新的活实例即自行退休，避免重复上报。
      if (supersededByNewerInstance(instanceID)) {
        dbg(`retire superseded instance=${instanceID}`)
        dispose()
        return
      }
      writeHeartbeat(ctx, instanceID)
      void processReplyJobs(ctx, instanceID)
    }, HEARTBEAT_INTERVAL_MS)

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

    return dispose
  },
}
