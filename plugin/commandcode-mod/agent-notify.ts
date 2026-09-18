/**
 * Agent-notify Command Code mod：一轮任务结束时调用 agent-notify.exe 推送微信消息。
 *
 * 形状：Command Code 的 mod 约定 —— 单文件 TypeScript，default 导出工厂 `(cmd) => void`，
 * jiti 免构建加载。为避免依赖未安装的 `@commandcode/harness` 类型包（会让 tsc --noEmit 失败），
 * 这里只声明本文件用到的 ModApi 子集。
 *
 * 触发：`run_end`（一次 run = 一个用户回合结束）。
 * 摘要：run_end 的 result.finalText。
 * 标题：`session_titled` 事件给出的会话标题，未拿到时由 CLI 用默认标题兜底。
 * 会话 ID：run_start/run_end 的 sessionId，用于建立引用路由。
 * 心跳：每 5 秒写一次 commandcode-reply-inbox/heartbeats/<实例>.json，供悬浮窗判断接入状态。
 * 安全：失败全部吞掉，永远不影响 Command Code 运行。
 *
 * 安装器会把安装目录里的绝对路径写进下面的 BAKED_BIN；仓库内副本保持空串，
 * 依次回退到环境变量、%USERPROFILE%\bin\agent-notify.exe 和 PATH。
 *
 * 环境变量：
 * - AGENT_NOTIFY_BIN：覆盖 agent-notify.exe 路径，优先级高于安装器写入的路径
 * - AGENT_NOTIFY_COMMANDCODE_MARKER_FILE：开关 marker，存在即停，默认
 *   %USERPROFILE%\.config\agent-notify\commandcode.off
 * - AGENT_NOTIFY_DRYRUN=1：只渲染不发送（联调用）
 * - AGENT_NOTIFY_OFF=1：mod 总开关，直接不拉起 CLI
 * - AGENT_NOTIFY_DEBUG=1：把判定与调用结果写进 %TEMP%\agent-notify\commandcode-debug.log
 *
 * 勿扰时段与标题免打扰由 CLI 统一判定，mod 不重复实现。
 */

// 归属校验标识：集成检测按它确认该文件由 AgentNotify 创建，勿删。
const MOD_MARKER = "agent-notify-commandcode-mod"

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

const REPLY_PENDING_DIR = `${REPLY_DIR}/pending`
const REPLY_PROCESSING_DIR = `${REPLY_DIR}/processing`
const REPLY_RESULT_DIR = `${REPLY_DIR}/results`
const REPLY_HEARTBEAT_DIR = `${REPLY_DIR}/heartbeats`
const DEBUG_LOG_FILE = `${TEMP_DIR}/commandcode-debug.log`
const PROBE_LOG_FILE = `${TEMP_DIR}/commandcode-probe.log`
const REPLY_WINDOW_POLL_MS = 500

const MILLISECONDS_PER_SECOND = 1000
const HEARTBEAT_MS = 5 * MILLISECONDS_PER_SECOND
const HEARTBEAT_STALE_SWEEP_MS = 60 * MILLISECONDS_PER_SECOND
const REPLY_PUMP_MS = 1 * MILLISECONDS_PER_SECOND
const REPLY_PROCESSING_STALE_MS = 30 * MILLISECONDS_PER_SECOND
const NOTIFY_CHILD_TIMEOUT_MS = 25 * MILLISECONDS_PER_SECOND
const NOTIFY_CALLBACK_TIMEOUT_MS = 30 * MILLISECONDS_PER_SECOND
const DEBUG_EXCERPT_CHARS = 200
const NOTIFY_MAX_CHARS = 0
const TITLE_MAX_CHARS = 40
const TRANSCRIPT_SCAN_MAX_BYTES = 2 * 1024 * 1024
const INSTANCE_RANDOM_LENGTH = 8
const PRIVATE_FILE_MODE = 0o600
const PRIVATE_DIRECTORY_MODE = 0o700

// 安装器会把引号里的值替换为实际安装路径；仓库内副本保持空串，走下面的探测链。
const BAKED_BIN = ""

type FsModule = typeof import("node:fs")
type JsonRecord = Record<string, unknown>

interface CommandCodeEvent {
  type?: string
  sessionId?: string
  title?: string
  result?: { finalText?: string; sessionId?: string }
}

interface StopInput {
  lastAssistantText?: string
  stopReason?: string
  turnNumber?: number
}

interface CommandCodeHooks {
  onStop?: (
    input: StopInput,
  ) =>
    | { continue?: boolean; reason?: string }
    | undefined
    | Promise<{ continue?: boolean; reason?: string } | undefined>
}

interface CommandCodeApi {
  on(event: string, handler: (event: CommandCodeEvent) => void): unknown
  hooks?(hooks: CommandCodeHooks): unknown
  /** 把消息注入当前会话；steer 在当前工具批次后落地，follow-up 只在 run 将结束时投递。 */
  queueMessage?(input: { content: string; deliverAs?: "steer" | "follow-up" }): void
}

interface ReplyJob {
  id: string
  sessionID: string
  text: string
  createdAt?: string
  expiresAt?: string
}

let fsMod: FsModule | null = null
let debugLog: ((message: string) => void) | null = null
let api: CommandCodeApi | null = null
// 当前 run 属于哪个会话。cmd.hooks/on 是进程级注册，每个会话实例各注册一份，
// 因此钩子里必须靠它判断"这个 run 是不是我的"，否则所有实例都会去挂住同一个 run。
let currentRunSessionId = ""

// Command Code 会为每个会话各调用一次 mod 工厂，所以状态必须"每实例一份"：
// 用模块级共享变量会让后运行的会话覆盖前面会话的 sessionId，心跳随之指向错误会话，
// 引用回复的就绪检查就会误判"目标会话未在运行"。
interface InstanceState {
  instanceID: string
  sessionId: string
  title: string
  runActive: boolean
  windowOpen: boolean
  injectedCount: number
  pendingReplyText: string
  pumpRunning: boolean
}

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
function resolveTarget(): string {
  const configured = envValue("AGENT_NOTIFY_BIN")
  if (configured) {
    return configured
  }
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
    } catch {
      fsMod = null
    }
  }
  return fsMod
}

function dbg(message: string): void {
  try {
    if (debugLog) {
      debugLog(message)
    }
  } catch {
    /* 调试日志失败不影响主流程 */
  }
}

/** 调试探针：只在 AGENT_NOTIFY_DEBUG=1 时写临时日志，便于排查窗口与投递。 */
function probeLog(message: string): void {
  try {
    if (envValue("AGENT_NOTIFY_DEBUG") !== "1" || !fsMod) {
      return
    }
    fsMod.appendFileSync(PROBE_LOG_FILE, `${new Date().toISOString()} ${message}\n`, {
      mode: PRIVATE_FILE_MODE,
    })
  } catch {
    /* 忽略 */
  }
}

function keepAliveEnabled(): boolean {
  return envValue("AGENT_NOTIFY_COMMANDCODE_KEEPALIVE") === "1"
}

/** AgentNotify 配置：引用回复开关与 CommandCode 回复窗口秒数（0 = 不等待）。 */
function agentNotifyConfig(): JsonRecord {
  const parsed = readJsonFile(CONFIG_FILE)
  return isRecord(parsed) ? parsed : {}
}

function replyFeatureEnabled(): boolean {
  if (envValue("AGENT_NOTIFY_REPLY_ENABLED") === "1") {
    return true
  }
  return agentNotifyConfig().replyEnabled === true
}

/**
 * 回复窗口秒数：0 表示不等待（默认，保守）。
 * 优先级：环境变量 > 配置文件 commandCodeReplyWindowSec。
 */
function windowSeconds(): number {
  const env = Number(envValue("AGENT_NOTIFY_COMMANDCODE_WINDOW_SEC"))
  if (Number.isFinite(env) && env > 0) {
    return Math.min(Math.floor(env), 600)
  }
  const configured = Number(agentNotifyConfig().commandCodeReplyWindowSec)
  if (Number.isFinite(configured) && configured > 0) {
    return Math.min(Math.floor(configured), 600)
  }
  return 0
}

/** 只有"功能开启 + 引用回复开启 + 该 Agent 未被暂停"时才撑窗口。 */
function replyWindowEnabled(): boolean {
  return windowSeconds() > 0 && replyFeatureEnabled() && !markerOff()
}

function sleepPromise(ms: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms))
}

function initDebugLog(): void {
  try {
    if (envValue("AGENT_NOTIFY_DEBUG") !== "1") {
      return
    }
    debugLog = (message: string) => {
      try {
        fsMod?.appendFileSync(
          DEBUG_LOG_FILE,
          `${new Date().toISOString()} ${MOD_MARKER} ${message}\n`,
          { mode: PRIVATE_FILE_MODE },
        )
      } catch {
        /* 忽略 */
      }
    }
  } catch {
    debugLog = null
  }
}

function ensureDirs(): void {
  if (!fsMod || !REPLY_DIR) {
    return
  }
  for (const dir of [REPLY_DIR, REPLY_PENDING_DIR, REPLY_PROCESSING_DIR, REPLY_RESULT_DIR, REPLY_HEARTBEAT_DIR]) {
    try {
      fsMod.mkdirSync(dir, { recursive: true, mode: PRIVATE_DIRECTORY_MODE })
      try {
        fsMod.chmodSync(dir, PRIVATE_DIRECTORY_MODE)
      } catch {
        /* Windows 或不支持 chmod 的文件系统保持原权限 */
      }
    } catch (error) {
      dbg(`ensure dir ${dir} fail: ${errorMessage(error)}`)
    }
  }
}

function writeFileAtomic(path: string, value: unknown): void {
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

function newInstanceID(): string {
  const random = Math.random()
    .toString(36)
    .slice(2, 2 + INSTANCE_RANDOM_LENGTH)
  return `${process.pid}-${Date.now().toString(36)}-${random}`
}

/** 清理僵死实例留下的心跳：进程被杀时不会有机会删除自己的文件。 */
function sweepStaleHeartbeats(keepName: string): void {
  if (!fsMod) {
    return
  }
  const now = Date.now()
  let names: string[] = []
  try {
    names = fsMod.readdirSync(REPLY_HEARTBEAT_DIR)
  } catch {
    return
  }
  for (const name of names) {
    if (!name.endsWith(".json") || name === keepName) {
      continue
    }
    try {
      if (now - fsMod.statSync(`${REPLY_HEARTBEAT_DIR}/${name}`).mtimeMs > HEARTBEAT_STALE_SWEEP_MS) {
        fsMod.unlinkSync(`${REPLY_HEARTBEAT_DIR}/${name}`)
      }
    } catch {
      /* 忽略 */
    }
  }
}

function writeHeartbeat(state: InstanceState): void {
  try {
    if (!fsMod || !REPLY_HEARTBEAT_DIR || !state.instanceID) {
      return
    }
    ensureDirs()
    writeFileAtomic(`${REPLY_HEARTBEAT_DIR}/${state.instanceID}.json`, {
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
  const timer = setInterval(
    () => writeHeartbeat(state),
    HEARTBEAT_MS,
  ) as unknown as { unref?: () => void }
  // headless（cmd -p）下不能让定时器拖住进程退出。
  try {
    if (typeof timer.unref === "function") {
      timer.unref()
    }
  } catch {
    /* 忽略 */
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
    writeFileAtomic(`${REPLY_RESULT_DIR}/${id}.json`, {
      ok,
      code: code || undefined,
      error: error || undefined,
    })
  } catch (err) {
    dbg(`write result fail: ${errorMessage(err)}`)
  }
}

function removeProcessingFile(id: string): void {
  try {
    fsMod?.unlinkSync(`${REPLY_PROCESSING_DIR}/${id}.json`)
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
    names = fsMod.readdirSync(REPLY_PROCESSING_DIR)
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
      stale = now - fsMod.statSync(`${REPLY_PROCESSING_DIR}/${name}`).mtimeMs > REPLY_PROCESSING_STALE_MS
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
    const detail = seconds > 0
      ? `回复窗口已过（通知发出后 ${seconds} 秒内可引用回复）`
      : "CommandCode 回复窗口未开启"
    writeReplyResult(job.id, false, "window_closed", detail)
    dbg(`inject rejected (window closed) id=${job.id}`)
    return
  }
  // 用 onStop 的 reason 携带用户正文送达模型：queueMessage 的 follow-up 在窗口内
  // 不会被 harness 消费（实测模型只看到声明、看不到正文），所以这里只暂存正文。
  state.pendingReplyText = state.pendingReplyText
    ? `${state.pendingReplyText}\n${job.text}`
    : job.text
  state.injectedCount += 1
  writeReplyResult(job.id, true, "", "")
  probeLog(`reply queued id=${job.id} sid=${job.sessionID} chars=${job.text.length}`)
  dbg(`reply queued id=${job.id} sid=${job.sessionID}`)
}

function pumpReplyJobs(state: InstanceState): void {
  if (state.pumpRunning || !fsMod) {
    return
  }
  state.pumpRunning = true
  try {
    recoverStaleProcessing()
    let names: string[] = []
    try {
      names = fsMod.readdirSync(REPLY_PENDING_DIR)
    } catch {
      return
    }
    for (const name of names) {
      if (!name.endsWith(".json")) {
        continue
      }
      const id = name.slice(0, -".json".length)
      const pendingPath = `${REPLY_PENDING_DIR}/${name}`
      const processingPath = `${REPLY_PROCESSING_DIR}/${name}`
      const job = readReplyJob(pendingPath)
      if (!job || jobExpired(job)) {
        try {
          fsMod.renameSync(pendingPath, processingPath)
        } catch {
          continue
        }
        writeReplyResult(id, false, "invalid_job", "任务无效或已过期")
        removeProcessingFile(id)
        continue
      }
      // queueMessage 只能注入自身会话：会话不匹配就留在 pending，交给正确的实例。
      if (!state.sessionId || job.sessionID !== state.sessionId) {
        continue
      }
      try {
        fsMod.renameSync(pendingPath, processingPath)
      } catch {
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

function startReplyPump(state: InstanceState): void {
  ensureDirs()
  pumpReplyJobs(state)
  const timer = setInterval(() => pumpReplyJobs(state), REPLY_PUMP_MS) as unknown as { unref?: () => void }
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

// Command Code 把会话存在 <home>/.commandcode/projects/<cwd slug>/<sessionId>.*；session id 唯一，
// 因此无需知道 slug 算法，直接扫描各项目目录即可定位。
function projectRoot(): string {
  return `${HOME_DIR}/.commandcode/projects`
}

function readJsonFile(path: string): unknown {
  try {
    if (!fsMod) return undefined
    return JSON.parse(fsMod.readFileSync(path, "utf8"))
  } catch {
    return undefined
  }
}

/** 读 <sessionId>.meta.json 的 title；拿不到返回空串。 */
function metaTitle(sessionID: string): string {
  if (!fsMod || !sessionID) return ""
  const root = projectRoot()
  try {
    if (!fsMod.existsSync(root)) return ""
    for (const entry of fsMod.readdirSync(root, { withFileTypes: true })) {
      if (!entry.isDirectory()) continue
      const meta = `${root}/${entry.name}/${sessionID}.meta.json`
      if (!fsMod.existsSync(meta)) continue
      const parsed = readJsonFile(meta)
      if (isRecord(parsed) && typeof parsed.title === "string" && parsed.title.trim()) {
        return parsed.title.trim()
      }
    }
  } catch {
    /* 忽略 */
  }
  return ""
}

/** 会话标题尚未生成时，用 transcript 首条用户消息兜底。 */
function transcriptTitle(sessionID: string): string {
  if (!fsMod || !sessionID) return ""
  const root = projectRoot()
  try {
    if (!fsMod.existsSync(root)) return ""
    for (const entry of fsMod.readdirSync(root, { withFileTypes: true })) {
      if (!entry.isDirectory()) continue
      const file = `${root}/${entry.name}/${sessionID}.jsonl`
      if (!fsMod.existsSync(file)) continue
      if (fsMod.statSync(file).size > TRANSCRIPT_SCAN_MAX_BYTES) continue
      for (const line of fsMod.readFileSync(file, "utf8").split("\n")) {
        const trimmed = line.trim()
        if (!trimmed) continue
        let parsed: unknown
        try {
          parsed = JSON.parse(trimmed)
        } catch {
          continue
        }
        if (!isRecord(parsed) || parsed.type !== "message") continue
        const message = parsed.message
        if (!isRecord(message) || message.role !== "user") continue
        const content = message.content
        if (!Array.isArray(content)) continue
        for (const part of content) {
          if (!isRecord(part) || part.type !== "text" || typeof part.text !== "string") continue
          const text = part.text.trim()
          if (!text) continue
          return text.split("\n")[0].trim().slice(0, TITLE_MAX_CHARS)
        }
      }
    }
  } catch {
    /* 忽略 */
  }
  return ""
}

/** 组装标题参数：优先 session_titled 事件，其次 meta.json，最后 transcript 首条用户消息。 */
function resolveRunTitle(state: InstanceState): string {
  const name = state.title || metaTitle(state.sessionId) || transcriptTitle(state.sessionId)
  return name ? `【CommandCode】${name}` : "【CommandCode】"
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
    "commandcode",
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
  const windowSec = windowSeconds()
  if (windowSec > 0) {
    // 让 AgentNotify 在通知页脚写明"多少秒内可引用回复"。
    args.push("--reply-window", String(windowSec))
  }
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

async function pushNotificationText(summary: string, state: InstanceState): Promise<void> {
  try {
    if (envValue("AGENT_NOTIFY_OFF") === "1") {
      dbg("skip: OFF=1")
      return
    }
    if (markerOff()) {
      dbg("skip: marker-off")
      return
    }
    await spawnNotify(resolveRunTitle(state), summary, state.sessionId)
  } catch (error) {
    dbg(`push fail: ${errorMessage(error)}`)
  }
}

async function pushNotification(
  event: CommandCodeEvent,
  state: InstanceState,
): Promise<void> {
  await pushNotificationText(extractFinalText(event), state)
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
    initDebugLog()
    ensureDirs()
    const state: InstanceState = {
      instanceID: newInstanceID(),
      sessionId: "",
      title: "",
      runActive: false,
      windowOpen: false,
      injectedCount: 0,
      pendingReplyText: "",
      pumpRunning: false,
    }
    startHeartbeat(state)
    startReplyPump(state)
    dbg(`loaded instance=${state.instanceID}`)
    probeLog(`instance loaded ${state.instanceID}`)

    safeOn(cmd, "session_start", () => writeHeartbeat(state))
    safeOn(cmd, "run_start", (event) => {
      const sessionID = resolveSessionId(event)
      if (sessionID) {
        state.sessionId = sessionID
      }
      state.runActive = true
      state.windowOpen = false
      state.pendingReplyText = ""
      currentRunSessionId = sessionID
      probeLog(`run_start sid=${state.sessionId} keepAlive=${keepAliveEnabled()} window=${replyWindowEnabled()} seconds=${windowSeconds()}`)
      writeHeartbeat(state)
    })
    safeOn(cmd, "session_titled", (event) => {
      if (typeof event.title === "string" && event.title) {
        state.title = event.title
      }
    })
    safeOn(cmd, "run_end", (event) => {
      state.runActive = false
      state.windowOpen = false
      currentRunSessionId = ""
      probeLog(`run_end sid=${resolveSessionId(event) || state.sessionId}`)
      if (replyWindowEnabled()) {
        // 回复窗口模式已在 onStop 推送，避免重复。
        return
      }
      void pushNotification(event, state)
    })

    // 实验：onStop 里先推送、再撑开窗口等微信引用回复；窗口内确实收到回复才续一轮让它落地，
    // 没收到就正常结束（零额外回合）。
    cmd.hooks?.({
      onStop: async (input) => {
        const lastText = typeof input?.lastAssistantText === "string" ? input.lastAssistantText : ""
        // 只处理属于本实例会话的 run：cmd.hooks 是进程级注册，所有会话实例都会收到同一次 onStop。
        if (!state.sessionId || state.sessionId !== currentRunSessionId) {
          return undefined
        }
        if (replyWindowEnabled()) {
          const seconds = windowSeconds()
          state.windowOpen = true
          const before = state.injectedCount
          probeLog(`onStop push sid=${state.sessionId} chars=${lastText.length} window=${seconds}s`)
          await pushNotificationText(lastText, state)
          // 轮询而不是睡满整个窗口：回复一到就立刻续跑，不必干等剩余时间。
          const deadline = Date.now() + seconds * MILLISECONDS_PER_SECOND
          try {
            while (Date.now() < deadline) {
              if (state.injectedCount > before) {
                break
              }
              await sleepPromise(REPLY_WINDOW_POLL_MS)
            }
          } finally {
            state.windowOpen = false
          }
          if (state.injectedCount > before && state.pendingReplyText) {
            const reply = state.pendingReplyText
            state.pendingReplyText = ""
            probeLog(`onStop continue with reply chars=${reply.length}`)
            // 用 reason 把用户正文送到模型：这是窗口内唯一可靠的投递通道。
            return {
              continue: true,
              reason: `微信引用回复（请把它当作新的用户指示处理）：\n${reply}`,
            }
          }
          probeLog("onStop stop (no reply in window)")
          return undefined
        }
        if (!keepAliveEnabled()) {
          probeLog("onStop stop")
          return undefined
        }
        // 保活只用于联调探针，默认关闭（会让会话持续产出填充回合）。
        probeLog("keepalive continue")
        return {
          continue: true,
          reason: 'AgentNotify keep-alive probe: reply with exactly "⏳" and take no other action.',
        }
      },
    })
  } catch (error) {
    dbg(`init fail: ${errorMessage(error)}`)
  }
}

export default function (cmd: CommandCodeApi): void {
  try {
    void start(cmd)
  } catch (error) {
    dbg(`start fail: ${errorMessage(error)}`)
  }
}
