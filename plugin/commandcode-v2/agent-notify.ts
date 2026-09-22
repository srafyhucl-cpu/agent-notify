/**
 * Agent-notify Command Code V2 mod。
 *
 * 形状：Command Code 的 mod 约定 —— 单文件 TypeScript，default 导出工厂 `(cmd) => void`，
 * jiti 免构建加载。为避免依赖未安装的 `@commandcode/harness` 类型包（会让 tsc --noEmit 失败），
 * 这里只声明本文件用到的 ModApi 子集。
 *
 * 触发：`run_end`（一次 run = 一个用户回合结束）向 agentnotify-ingress.exe 提交
 * protocolVersion=1 的 agent.event；标题优先取 `session_titled`，meta.json 与 transcript
 * 兜底由适配器完成。
 *
 * 回复窗口：`commandCodeReplyWindowSec`（0 = 关闭，默认）大于 0 时，onStop 会先推送通知，
 * 再在本地等待回复任务（等待期间不消耗 token）；窗口内收到引用回复就用 Stop hook 的
 * `reason` 把正文作为新的用户指示送进模型。窗口结束后到达的任务一律明确失败，绝不留到
 * 下一次 run。窗口秒数优先级：环境变量 > 应用写入的 `commandcode-reply-inbox/window.json`
 * （界面里保存的值，适配器同源写出）> 旧 JSON 配置——老用户只配了旧文件仍然生效。
 *
 * 心跳：每 5 秒写一次 commandcode-reply-inbox/heartbeats/<实例>.json，供适配器判断
 * “目标会话是否在线、窗口是否开着”；pending → processing 用原子 rename，任务只由持有
 * 该会话的实例认领。
 *
 * 安全：所有失败都被吞掉并写诊断，永远不影响 Command Code。
 *
 * 安装器会把安装目录里的绝对路径写进下面的 BAKED_INGRESS；仓库内副本保持空串，
 * 依次回退到环境变量、%USERPROFILE%\bin\agentnotify-ingress.exe 和 PATH。
 *
 * 环境变量：
 * - AGENT_NOTIFY_INGRESS_BIN：覆盖 ingress 路径，优先级高于安装器写入的路径
 * - AGENT_NOTIFY_COMMANDCODE_MARKER_FILE：开关 marker，存在即停，默认
 *   %USERPROFILE%\.config\agent-notify\commandcode.off
 * - AGENT_NOTIFY_COMMANDCODE_REPLY_DIR：回复收件箱目录（应用写入的 window.json 也在里面）
 * - AGENT_NOTIFY_COMMANDCODE_WINDOW_SEC：回复窗口秒数（0 = 关闭，最大 600）
 * - AGENT_NOTIFY_CONFIG_FILE / AGENT_NOTIFY_CONFIG_DIR：AgentNotify 配置位置
 * - AGENT_NOTIFY_OFF=1：mod 总开关，直接不提交事件
 * - AGENT_NOTIFY_DEBUG=1：把判定与调用结果写进 %TEMP%\agent-notify\commandcode-debug.log（有界）
 *
 * 勿扰时段与标题免打扰由 AgentNotify 统一判定，mod 不重复实现。
 */

// 归属校验标识：安装器按它确认该文件由 AgentNotify 创建，勿删。
const MOD_MARKER = "agent-notify-commandcode-mod"
const AGENT_ID = "commandcode"
const PROTOCOL_VERSION = 1
const EVENT_KIND = "agent.event"
const RUN_END_EVENT = "run_end"
const DEFAULT_BODY = "任务已完成。"
/** 与适配器 `MAX_REPLY_WINDOW_SEC` 保持一致。 */
const MAX_REPLY_WINDOW_SEC = 600
const WINDOW_ENV = "AGENT_NOTIFY_COMMANDCODE_WINDOW_SEC"
const WINDOW_CONFIG_KEY = "commandCodeReplyWindowSec"

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

const CONFIG_DIR =
  (typeof process !== "undefined" && process.env.AGENT_NOTIFY_CONFIG_DIR) ||
  (HOME_DIR ? `${HOME_DIR}/.config/agent-notify` : "")

const CONFIG_FILE =
  (typeof process !== "undefined" && process.env.AGENT_NOTIFY_CONFIG_FILE) ||
  (CONFIG_DIR ? `${CONFIG_DIR}/config.json` : "")

const MARKER_FILE =
  (typeof process !== "undefined" &&
    process.env.AGENT_NOTIFY_COMMANDCODE_MARKER_FILE) ||
  (CONFIG_DIR ? `${CONFIG_DIR}/commandcode.off` : "")

const REPLY_DIR =
  (typeof process !== "undefined" &&
    process.env.AGENT_NOTIFY_COMMANDCODE_REPLY_DIR) ||
  (CONFIG_DIR ? `${CONFIG_DIR}/commandcode-reply-inbox` : "")

const PENDING_DIR = `${REPLY_DIR}/pending`
const PROCESSING_DIR = `${REPLY_DIR}/processing`
const RESULT_DIR = `${REPLY_DIR}/results`
const HEARTBEAT_DIR = `${REPLY_DIR}/heartbeats`
/** 应用（界面保存时）写出的窗口值；mod 只读，绝不改。 */
const WINDOW_FILE = REPLY_DIR ? `${REPLY_DIR}/window.json` : ""
const DEBUG_LOG_FILE = `${TEMP_DIR}/commandcode-debug.log`

const MILLISECONDS_PER_SECOND = 1000
const HEARTBEAT_MS = 5 * MILLISECONDS_PER_SECOND
/** 僵死心跳清理阈值：进程被杀时不会有机会删除自己的心跳文件。 */
const HEARTBEAT_STALE_SWEEP_MS = 60 * MILLISECONDS_PER_SECOND
const REPLY_PUMP_MS = 1 * MILLISECONDS_PER_SECOND
/** 认领后崩溃的任务只报告失败，绝不自动重放。 */
const REPLY_PROCESSING_STALE_MS = 30 * MILLISECONDS_PER_SECOND
/** 轮询而不是睡满窗口：回复一到就立刻续跑。 */
const WINDOW_POLL_MS = 500
const INGRESS_CHILD_TIMEOUT_MS = 25 * MILLISECONDS_PER_SECOND
const INGRESS_CALLBACK_TIMEOUT_MS = 30 * MILLISECONDS_PER_SECOND
/** ingress 的正文硬上限是 64 KiB，留出转义余量后再截断。 */
const BODY_MAX_BYTES = 60 * 1024
const BODY_TRUNCATED_SUFFIX = "…（正文过长已截断）"
const ERROR_MAX_CHARS = 300
const DEBUG_LOG_MAX_BYTES = 512 * 1024
const PRIVATE_FILE_MODE = 0o600
const PRIVATE_DIRECTORY_MODE = 0o700

// 安装器会把引号里的值替换为实际安装路径；仓库内副本保持空串，走下面的探测链。
const BAKED_INGRESS = ""

type FsModule = typeof import("node:fs")
type JsonRecord = Record<string, unknown>

interface CommandCodeRunResult {
  finalText?: string
  sessionId?: string
  stopReason?: string
  turnCount?: number
}

interface CommandCodeEvent {
  type?: string
  sessionId?: string
  title?: string
  result?: CommandCodeRunResult
}

interface StopInput {
  lastAssistantText?: string
  stopReason?: string
  turnNumber?: number
}

interface StopOutput {
  continue?: boolean
  reason?: string
}

interface CommandCodeHooks {
  onStop?: (input: StopInput) => StopOutput | undefined | Promise<StopOutput | undefined>
}

interface CommandCodeUi {
  notify?: (message: string) => unknown
}

interface CommandCodeApi {
  on(event: string, handler: (event: CommandCodeEvent) => void): unknown
  hooks?(hooks: CommandCodeHooks): unknown
  ui?: CommandCodeUi
}

interface ReplyJob {
  id: string
  sessionID: string
  text: string
  createdAt?: string
  expiresAt?: string
}

// Command Code 会为每个会话各调用一次 mod 工厂，所以状态必须“每实例一份”：
// 模块级共享变量会让后运行的会话覆盖前面会话的 sessionId，心跳随之指向错误会话，
// 引用回复的就绪检查就会误判“目标会话未在运行”。
interface InstanceState {
  instanceID: string
  sessionId: string
  title: string
  windowOpen: boolean
  injectedCount: number
  pendingReplyText: string
  /** 本轮 run 是否已经推送过通知：避免 onStop 与 run_end 重复推送。 */
  pushedThisRun: boolean
  pumpRunning: boolean
}

let fsMod: FsModule | null = null
let api: CommandCodeApi | null = null
// 当前 run 属于哪个会话。cmd.hooks/on 是进程级注册，每个会话实例各注册一份，
// 因此钩子里必须靠它判断“这个 run 是不是我的”，否则所有实例都会去挂住同一个 run。
let currentRunSessionId = ""

function envValue(name: string): string {
  return (typeof process !== "undefined" && process.env[name]) || ""
}

function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error)
}

function isRecord(value: unknown): value is JsonRecord {
  return typeof value === "object" && value !== null
}

function existsInFs(path: string): boolean {
  try {
    return Boolean(fsMod && path && fsMod.existsSync(path))
  } catch {
    return false
  }
}

function markerOff(): boolean {
  return existsInFs(MARKER_FILE)
}

/** 安装器写入的绝对路径优先于 %USERPROFILE%\bin 默认值，自定义安装目录才不会失联。 */
function resolveIngressTarget(): string {
  const configured = envValue("AGENT_NOTIFY_INGRESS_BIN")
  if (configured) {
    return configured
  }
  if (existsInFs(BAKED_INGRESS)) {
    return BAKED_INGRESS
  }
  const home = envValue("USERPROFILE")
  if (home) {
    const installed = `${home}\\bin\\agentnotify-ingress.exe`
    if (existsInFs(installed)) {
      return installed
    }
  }
  return BAKED_INGRESS || "agentnotify-ingress.exe"
}

async function fsAsync(): Promise<FsModule | null> {
  if (!fsMod) {
    try {
      fsMod = await import("node:fs")
    } catch {
      fsMod = null
    }
  }
  return fsMod
}

/** 诊断日志有界：超过上限时清空重写，只保留最近一次失败，避免无界增长。 */
function dbg(message: string): void {
  try {
    if (envValue("AGENT_NOTIFY_DEBUG") !== "1" || !fsMod || !DEBUG_LOG_FILE) {
      return
    }
    fsMod.mkdirSync(TEMP_DIR, { recursive: true, mode: PRIVATE_DIRECTORY_MODE })
    if (
      fsMod.existsSync(DEBUG_LOG_FILE) &&
      fsMod.statSync(DEBUG_LOG_FILE).size >= DEBUG_LOG_MAX_BYTES
    ) {
      fsMod.writeFileSync(DEBUG_LOG_FILE, "", { mode: PRIVATE_FILE_MODE })
    }
    fsMod.appendFileSync(
      DEBUG_LOG_FILE,
      `${new Date().toISOString()} ${MOD_MARKER} ${message}\n`,
      { mode: PRIVATE_FILE_MODE },
    )
  } catch {
    /* 调试日志失败不影响主流程 */
  }
}

function sleepPromise(ms: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms))
}

function agentNotifyConfig(): JsonRecord {
  const parsed = readJsonFile(CONFIG_FILE)
  return isRecord(parsed) ? parsed : {}
}

/**
 * 回复窗口秒数：0 表示不等待（默认，保守）。
 * 优先级：环境变量（只有大于 0 才覆盖）> 收件箱 window.json（应用写的界面值）
 * > 旧 JSON 配置 `commandCodeReplyWindowSec`（老用户向后兼容）。
 * window.json 存在且是合法数字时按显式配置处理（0 = 界面关闭），不再回退旧配置；
 * 否则会出现 mod 开窗、适配器判定关闭的走偏。读取失败/非法只写诊断并回退下一优先级。
 */
function windowSeconds(): number {
  const env = Number(envValue(WINDOW_ENV))
  if (Number.isFinite(env) && env > 0) {
    return Math.min(Math.floor(env), MAX_REPLY_WINDOW_SEC)
  }
  const inbox = inboxWindowSeconds()
  if (inbox !== null) {
    return inbox
  }
  const configured = Number(agentNotifyConfig()[WINDOW_CONFIG_KEY])
  if (Number.isFinite(configured) && configured > 0) {
    return Math.min(Math.floor(configured), MAX_REPLY_WINDOW_SEC)
  }
  return 0
}

/**
 * 读应用写入的 window.json；缺失/损坏返回 null 并写诊断（原因进 commandcode-debug.log）。
 * 非数字值不放行：宁可回退旧配置，也不猜测目标。
 */
function inboxWindowSeconds(): number | null {
  if (!fsMod || !WINDOW_FILE || !existsInFs(WINDOW_FILE)) {
    return null
  }
  let raw = ""
  try {
    raw = fsMod.readFileSync(WINDOW_FILE, "utf8")
  } catch (error) {
    dbg(`window file read fail: ${errorMessage(error)}`)
    return null
  }
  let parsed: unknown = undefined
  try {
    parsed = JSON.parse(raw)
  } catch (error) {
    dbg(`window file invalid json: ${errorMessage(error)}`)
    return null
  }
  const value = isRecord(parsed) ? parsed[WINDOW_CONFIG_KEY] : undefined
  if (typeof value !== "number" || !Number.isFinite(value) || value < 0) {
    dbg(`window file invalid value: ${JSON.stringify(value)}`)
    return null
  }
  return Math.min(Math.floor(value), MAX_REPLY_WINDOW_SEC)
}

/** 该 Agent 被 marker 暂停时也不撑窗口。 */
function replyWindowEnabled(): boolean {
  return windowSeconds() > 0 && !markerOff()
}

function ensureDirs(): void {
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
    try {
      fsMod.mkdirSync(directory, { recursive: true, mode: PRIVATE_DIRECTORY_MODE })
      try {
        fsMod.chmodSync(directory, PRIVATE_DIRECTORY_MODE)
      } catch {
        /* Windows 或不支持 chmod 的文件系统保持原权限 */
      }
    } catch (error) {
      dbg(`ensure dir ${directory} fail: ${errorMessage(error)}`)
    }
  }
}

function writeFileAtomic(path: string, value: unknown): void {
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
        /* 描述符已关闭 */
      }
    }
    try {
      fsMod.unlinkSync(temporary)
    } catch {
      /* 临时文件已不存在 */
    }
    throw error
  }
}

function newInstanceID(): string {
  const random = Math.random().toString(36).slice(2, 10)
  return `${process.pid}-${Date.now().toString(36)}-${random}`
}

function createInstanceState(): InstanceState {
  return {
    instanceID: newInstanceID(),
    sessionId: "",
    title: "",
    windowOpen: false,
    injectedCount: 0,
    pendingReplyText: "",
    pushedThisRun: false,
    pumpRunning: false,
  }
}

/** 清理僵死实例留下的心跳：进程被杀时不会有机会删除自己的文件。 */
function sweepStaleHeartbeats(keepName: string): void {
  if (!fsMod) {
    return
  }
  const now = Date.now()
  let names: string[] = []
  try {
    names = fsMod.readdirSync(HEARTBEAT_DIR)
  } catch {
    return
  }
  for (const name of names) {
    if (!name.endsWith(".json") || name === keepName) {
      continue
    }
    try {
      if (
        now - fsMod.statSync(`${HEARTBEAT_DIR}/${name}`).mtimeMs >
        HEARTBEAT_STALE_SWEEP_MS
      ) {
        fsMod.unlinkSync(`${HEARTBEAT_DIR}/${name}`)
      }
    } catch {
      /* 忽略 */
    }
  }
}

function writeHeartbeat(state: InstanceState): void {
  try {
    if (!fsMod || !HEARTBEAT_DIR || !state.instanceID) {
      return
    }
    ensureDirs()
    writeFileAtomic(`${HEARTBEAT_DIR}/${state.instanceID}.json`, {
      ready: true,
      timestamp: new Date().toISOString(),
      sessionId: state.sessionId,
      windowOpen: state.windowOpen,
    })
    sweepStaleHeartbeats(`${state.instanceID}.json`)
  } catch (error) {
    dbg(`heartbeat fail: ${errorMessage(error)}`)
  }
}

function startHeartbeat(state: InstanceState): void {
  writeHeartbeat(state)
  const timer = setInterval(() => writeHeartbeat(state), HEARTBEAT_MS) as unknown as {
    unref?: () => void
  }
  // headless（cmd -p）下不能让定时器拖住进程退出。
  try {
    if (typeof timer.unref === "function") {
      timer.unref()
    }
  } catch {
    /* 忽略 */
  }
}

function readJsonFile(path: string): unknown {
  try {
    if (!fsMod || !path) {
      return undefined
    }
    return JSON.parse(fsMod.readFileSync(path, "utf8"))
  } catch {
    return undefined
  }
}

function readReplyJob(path: string): ReplyJob | null {
  const parsed = readJsonFile(path)
  if (!isRecord(parsed)) {
    return null
  }
  const id = typeof parsed.id === "string" ? parsed.id : ""
  const sessionID = typeof parsed.sessionID === "string" ? parsed.sessionID : ""
  const text = typeof parsed.text === "string" ? parsed.text : ""
  if (!id || !sessionID || !text) {
    return null
  }
  return {
    id,
    sessionID,
    text,
    createdAt: typeof parsed.createdAt === "string" ? parsed.createdAt : undefined,
    expiresAt: typeof parsed.expiresAt === "string" ? parsed.expiresAt : undefined,
  }
}

function jobExpired(job: ReplyJob): boolean {
  if (!job.expiresAt) {
    return false
  }
  const expires = Date.parse(job.expiresAt)
  return Number.isFinite(expires) && expires <= Date.now()
}

function writeReplyResult(id: string, ok: boolean, code: string, error: string): void {
  try {
    if (!fsMod) {
      return
    }
    writeFileAtomic(`${RESULT_DIR}/${id}.json`, {
      ok,
      code: code || undefined,
      error: error ? error.slice(0, ERROR_MAX_CHARS) : undefined,
    })
  } catch (error) {
    dbg(`write result fail: ${errorMessage(error)}`)
  }
}

function removeProcessingFile(id: string): void {
  try {
    fsMod?.unlinkSync(`${PROCESSING_DIR}/${id}.json`)
  } catch {
    /* 文件已不存在 */
  }
}

/** 处理中断（本实例崩溃/退出）的任务只报告失败，绝不自动重放，保持至多一次语义。 */
function recoverStaleProcessing(): void {
  if (!fsMod) {
    return
  }
  const now = Date.now()
  let names: string[] = []
  try {
    names = fsMod.readdirSync(PROCESSING_DIR)
  } catch {
    return
  }
  for (const name of names) {
    if (!name.endsWith(".json")) {
      continue
    }
    const id = name.slice(0, -".json".length)
    let stale = false
    try {
      stale =
        now - fsMod.statSync(`${PROCESSING_DIR}/${name}`).mtimeMs >
        REPLY_PROCESSING_STALE_MS
    } catch {
      stale = true
    }
    if (!stale) {
      continue
    }
    writeReplyResult(id, false, "inject_failed", "处理中断，未确认注入结果")
    removeProcessingFile(id)
  }
}

function injectReplyJob(job: ReplyJob, state: InstanceState): void {
  // 窗口没开就没有投递时机：必须明确失败，绝不能收下后石沉大海。
  if (!state.windowOpen) {
    const seconds = windowSeconds()
    const detail =
      seconds > 0
        ? `回复窗口已过（通知发出后 ${seconds} 秒内可引用回复）`
        : "CommandCode 回复窗口未开启"
    writeReplyResult(job.id, false, "window_closed", detail)
    dbg(`inject rejected (window closed) id=${job.id}`)
    return
  }
  // 用 onStop 的 reason 携带用户正文送达模型：窗口内的 queueMessage 不会在下一次
  // 模型调用前被消费（实测模型只看到声明、看不到正文），所以这里只暂存正文。
  state.pendingReplyText = state.pendingReplyText
    ? `${state.pendingReplyText}\n${job.text}`
    : job.text
  state.injectedCount += 1
  writeReplyResult(job.id, true, "", "")
  dbg(`reply queued id=${job.id} sid=${job.sessionID} chars=${job.text.length}`)
}

function pumpReplyJobs(state: InstanceState): void {
  if (state.pumpRunning || !fsMod) {
    return
  }
  state.pumpRunning = true
  try {
    // 收件箱目录被外部清掉时也要能自愈，否则任务会一直留在 pending。
    ensureDirs()
    recoverStaleProcessing()
    let names: string[] = []
    try {
      names = fsMod.readdirSync(PENDING_DIR)
    } catch {
      return
    }
    for (const name of names) {
      if (!name.endsWith(".json")) {
        continue
      }
      const id = name.slice(0, -".json".length)
      const pendingPath = `${PENDING_DIR}/${name}`
      const processingPath = `${PROCESSING_DIR}/${name}`
      const job = readReplyJob(pendingPath)
      if (!job || jobExpired(job)) {
        // 无效或过期的任务对哪个实例都已死：谁先看到谁清理，避免 pending 无限堆积。
        if (claimJob(pendingPath, processingPath)) {
          writeReplyResult(id, false, "invalid_job", "任务无效或已过期")
          removeProcessingFile(id)
        }
        continue
      }
      // 只认领持有该会话的实例的任务：会话不匹配就留在 pending，绝不改投。
      if (!state.sessionId || job.sessionID !== state.sessionId) {
        continue
      }
      if (!claimJob(pendingPath, processingPath)) {
        continue
      }
      injectReplyJob(job, state)
      removeProcessingFile(id)
    }
  } catch (error) {
    dbg(`reply pump fail: ${errorMessage(error)}`)
  } finally {
    state.pumpRunning = false
  }
}

/** pending → processing 用原子 rename，保证任务至多被一个实例认领。 */
function claimJob(pendingPath: string, processingPath: string): boolean {
  try {
    fsMod?.renameSync(pendingPath, processingPath)
    return true
  } catch {
    return false
  }
}

function startReplyPump(state: InstanceState): void {
  ensureDirs()
  pumpReplyJobs(state)
  const timer = setInterval(() => pumpReplyJobs(state), REPLY_PUMP_MS) as unknown as {
    unref?: () => void
  }
  try {
    if (typeof timer.unref === "function") {
      timer.unref()
    }
  } catch {
    /* 忽略 */
  }
}

function resolveSessionId(event: CommandCodeEvent): string {
  if (typeof event.sessionId === "string" && event.sessionId) {
    return event.sessionId
  }
  const result = event.result
  if (isRecord(result) && typeof result.sessionId === "string") {
    return result.sessionId
  }
  return ""
}

function extractFinalText(event: CommandCodeEvent): string {
  const result = event.result
  if (isRecord(result) && typeof result.finalText === "string") {
    return result.finalText
  }
  return ""
}

/** ingress 的 body 硬上限是 64 KiB，超限会整条拒收，因此按 UTF-8 字节截断。 */
function truncateBody(value: string): string {
  const text = value.trim()
  if (!text) {
    return ""
  }
  const bytes = Buffer.from(text, "utf8")
  if (bytes.length <= BODY_MAX_BYTES) {
    return text
  }
  return (
    bytes.subarray(0, BODY_MAX_BYTES).toString("utf8") + BODY_TRUNCATED_SUFFIX
  )
}

function buildRunEndPayload(
  sessionId: string,
  title: string,
  body: string,
): JsonRecord {
  const payload: JsonRecord = {
    eventType: RUN_END_EVENT,
    sessionId,
    body: truncateBody(body) || DEFAULT_BODY,
  }
  const trimmedTitle = title.trim()
  if (trimmedTitle) {
    payload.title = trimmedTitle
  }
  return payload
}

function buildRunEndEnvelope(payload: JsonRecord): JsonRecord {
  return {
    protocolVersion: PROTOCOL_VERSION,
    kind: EVENT_KIND,
    requestId: crypto.randomUUID(),
    agentId: AGENT_ID,
    payload,
  }
}

/** 提交事件给 ingress；失败只写诊断，绝不抛出、绝不阻塞 Command Code。 */
function submitEvent(envelope: JsonRecord): Promise<void> {
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
        const target = resolveIngressTarget()
        dbg(`ingress submit target=${target} bytes=${JSON.stringify(envelope).length}`)
        const child = execFile(
          target,
          [],
          { timeout: INGRESS_CHILD_TIMEOUT_MS, windowsHide: true },
          (error, _stdout, stderr) => {
            clearTimeout(timer)
            dbg(
              `ingress exit err=${error ? errorMessage(error) : "none"} stderr=${String(stderr || "").slice(0, ERROR_MAX_CHARS)}`,
            )
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

async function pushRunEnd(state: InstanceState, body: string, sessionId: string): Promise<void> {
  try {
    if (envValue("AGENT_NOTIFY_OFF") === "1") {
      dbg("skip: OFF=1")
      return
    }
    if (markerOff()) {
      dbg("skip: marker-off")
      return
    }
    const payload = buildRunEndPayload(sessionId || state.sessionId, state.title, body)
    await submitEvent(buildRunEndEnvelope(payload))
  } catch (error) {
    dbg(`push fail: ${errorMessage(error)}`)
  }
}

/** 等待窗口期间在 TUI 明确提示，避免用户以为会话卡住。 */
function notifyWaiting(state: InstanceState, seconds: number): void {
  try {
    api?.ui?.notify?.(
      `AgentNotify：正在等待微信引用回复（最多 ${seconds} 秒，等待期间不消耗 token）…`,
    )
  } catch (error) {
    dbg(`ui notify fail: ${errorMessage(error)}`)
  }
  dbg(`window open sid=${state.sessionId} seconds=${seconds}`)
}

function safeOn(
  cmd: CommandCodeApi,
  event: string,
  handler: (event: CommandCodeEvent) => void,
): void {
  try {
    cmd.on(event, (payload) => {
      try {
        handler(payload || {})
      } catch (error) {
        dbg(`handler ${event} fail: ${errorMessage(error)}`)
      }
    })
  } catch (error) {
    dbg(`on ${event} fail: ${errorMessage(error)}`)
  }
}

async function start(cmd: CommandCodeApi): Promise<void> {
  try {
    api = cmd
    await fsAsync()
    ensureDirs()
    const state = createInstanceState()
    startHeartbeat(state)
    startReplyPump(state)
    dbg(`loaded instance=${state.instanceID}`)

    safeOn(cmd, "session_start", () => writeHeartbeat(state))
    safeOn(cmd, "run_start", (event) => {
      const sessionID = resolveSessionId(event)
      if (sessionID) {
        state.sessionId = sessionID
      }
      state.windowOpen = false
      state.pendingReplyText = ""
      state.pushedThisRun = false
      currentRunSessionId = sessionID
      writeHeartbeat(state)
    })
    safeOn(cmd, "session_titled", (event) => {
      if (typeof event.title === "string" && event.title) {
        state.title = event.title
      }
    })
    safeOn(cmd, "run_end", (event) => {
      const sessionID = resolveSessionId(event)
      // 同一进程里的其他会话实例也会收到这次 run_end：不属于本实例的会话就交给它的持有者。
      if (sessionID && state.sessionId && sessionID !== state.sessionId) {
        return
      }
      state.windowOpen = false
      if (currentRunSessionId === sessionID) {
        currentRunSessionId = ""
      }
      // onStop 已经推送过（窗口模式）就不再重复推送；硬停止（max_turns、
      // terminate、interrupted）没经过 onStop，这里必须补上。
      if (state.pushedThisRun) {
        state.pushedThisRun = false
        return
      }
      void pushRunEnd(state, extractFinalText(event), sessionID)
    })

    // 窗口模式：onStop 里先推送、再撑开窗口等微信引用回复；窗口内确实收到回复才
    // 续一轮让它落地，没收到就正常结束（零额外回合）。
    cmd.hooks?.({
      onStop: async (input) => {
        const lastText =
          typeof input?.lastAssistantText === "string" ? input.lastAssistantText : ""
        // 只处理属于本实例会话的 run：cmd.hooks 是进程级注册，所有会话实例都会收到同一次 onStop。
        if (!state.sessionId || state.sessionId !== currentRunSessionId) {
          return undefined
        }
        if (!replyWindowEnabled()) {
          return undefined
        }
        const seconds = windowSeconds()
        // 必须在开窗前取基线：回复泵可能抢在赋值前完成注入，否则会白等满整个窗口。
        const before = state.injectedCount
        state.windowOpen = true
        state.pushedThisRun = true
        // 立刻刷心跳：适配器按心跳里的 windowOpen 判断窗口是否开着，等 5 秒周期会误报“窗口已过”。
        writeHeartbeat(state)
        notifyWaiting(state, seconds)
        await pushRunEnd(state, lastText, state.sessionId)
        const deadline = Date.now() + seconds * MILLISECONDS_PER_SECOND
        try {
          while (Date.now() < deadline) {
            if (state.injectedCount > before) {
              break
            }
            await sleepPromise(WINDOW_POLL_MS)
          }
        } finally {
          state.windowOpen = false
          writeHeartbeat(state)
        }
        if (state.injectedCount > before && state.pendingReplyText) {
          const reply = state.pendingReplyText
          state.pendingReplyText = ""
          // 续跑后的下一轮结束时由 onStop（或 run_end 兜底）再推送，不算本轮已推送。
          state.pushedThisRun = false
          dbg(`onStop continue chars=${reply.length}`)
          // 用 reason 把用户正文送到模型：这是窗口内唯一可靠的投递通道。
          return {
            continue: true,
            reason: `微信引用回复（请把它当作新的用户指示处理）：\n${reply}`,
          }
        }
        dbg("onStop stop (no reply in window)")
        return undefined
      },
    })
  } catch (error) {
    dbg(`init fail: ${errorMessage(error)}`)
  }
}

const __test = {
  buildRunEndEnvelope,
  buildRunEndPayload,
  createInstanceState,
  injectReplyJob,
  // 与 setup 一样先把 node:fs 载入：纯函数只有拿到 fs 才能读配置与收件箱。
  loadFs: fsAsync,
  pumpReplyJobs,
  pushRunEnd,
  replyWindowEnabled,
  resolveSessionId,
  truncateBody,
  windowSeconds,
  writeHeartbeat,
}

export { __test }
export default function (cmd: CommandCodeApi): void {
  try {
    void start(cmd)
  } catch (error) {
    dbg(`start fail: ${errorMessage(error)}`)
  }
}
