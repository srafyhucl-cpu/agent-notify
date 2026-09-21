"use strict"

const fs = require("node:fs")
const path = require("node:path")
const vscode = require("vscode")

// Devin 桌面端把消息写回会话的通道：ACP 会话直接向桌面端常驻的
// `devin.exe acp` 子进程写 session/prompt，聊天面板只做尽力激活；旧 Cascade
// 会话保留精确直发和聊天面板回退。整个过程都在桌面端内完成，不依赖 CLI 登录。
//
// V2 与旧扩展的差异：任务必须携带适配器解析好的桌面端 Cascade 标识
// （targetID），扩展只按显式标识投递；缺失时直接判定任务无效，不回退到
// sessionID 或最近会话。扩展同样不访问 SQLite 与 Credential Manager，
// 所有任务都来自 Agent-notify 适配器写入的本地收件箱。
const {
  ACP_ERROR_CODES,
  ACP_CHANNEL_MISSING_MESSAGE,
  DEFAULT_WRITE_TIMEOUT_MS,
  createACPStdioBridge,
} = require("./acp-bridge")
const CHAT_ACTION_COMMAND = "devin.sendChatActionMessage"
const OPEN_ACP_SESSION_COMMAND = "devin.prioritized.openAgentInSmartPane"
const OPEN_LEGACY_SESSION_ACTION = "openCascadeIdInChatPanel"
const SEND_INPUT_ACTION = "sendCascadeInput"
// 旧 Cascade 的精确通道直接按 Cascade 标识提交消息；仅在桌面端未提供该
// 命令时才退回“打开面板 + 提交输入”。
const DIRECT_SEND_COMMAND = "windsurf2plus.remote.sendMessage"
// 精确打开 ACP 会话后，编辑器仍要异步加载历史；过早提交会被丢弃。
const SESSION_OPEN_SETTLE_MS = 1000
const ACP_SESSION_PREFIX = "acp/"

function isACPSession(targetID) {
  return targetID.slice(0, ACP_SESSION_PREFIX.length).toLowerCase() === ACP_SESSION_PREFIX
}

// resolveACPSessionID 取 ACP 请求使用的会话号；桌面端标识带命名空间时去掉前缀。
function resolveACPSessionID(sessionID) {
  if (!isACPSession(sessionID)) {
    return sessionID
  }
  return sessionID.slice(ACP_SESSION_PREFIX.length).replace(/^devin-cli\//i, "")
}
const MAX_REPLY_TEXT_CHARS = 20000

const HOME_DIR = process.env.USERPROFILE || process.env.HOME || ""
const CONFIG_DIR =
  process.env.AGENT_NOTIFY_CONFIG_DIR ||
  (HOME_DIR ? path.join(HOME_DIR, ".config", "agent-notify") : "")
const REPLY_DIR =
  process.env.AGENT_NOTIFY_DEVIN_REPLY_DIR ||
  (CONFIG_DIR ? path.join(CONFIG_DIR, "devin-reply-inbox") : "")

const PRIVATE_DIR_MODE = 0o700
const PRIVATE_FILE_MODE = 0o600
const HEARTBEAT_INTERVAL_MS = 5000
const PUMP_INTERVAL_MS = 250
const PROCESSING_STALE_MS = 90000
const RESULT_MAX_CHARS = 500
const JOB_ID_PATTERN = /^[0-9a-f]{32}$/
// 桌面端 Cascade 标识带命名空间（例如 acp/devin-cli/<会话号>），因此允许斜杠。
const SESSION_ID_PATTERN = /^[a-z0-9][a-z0-9._/-]{1,191}$/i

const CODE_DESKTOP_UNAVAILABLE = "desktop_unavailable"
const CODE_INVALID_JOB = "invalid_job"
const CODE_SESSION_NOT_FOUND = "session_not_found"
const CODE_TURN_FAILED = "turn_failed"

const DESKTOP_UNAVAILABLE_MESSAGE =
  "当前 Devin 桌面端未提供精确回复能力，请更新 Devin 桌面端后重启；回复不会改投到新会话"
const MISSING_TARGET_MESSAGE =
  "引用回复缺少精确会话标识，请重新引用原通知后再试；回复不会改投到最近会话"

let activeInstance = null

function ensurePrivateDirectory(directory) {
  fs.mkdirSync(directory, { recursive: true, mode: PRIVATE_DIR_MODE })
  try {
    fs.chmodSync(directory, PRIVATE_DIR_MODE)
  } catch {
    // Windows 文件系统的 chmod 语义有限，保持原权限即可。
  }
}

function writeJSONAtomic(target, value) {
  const temporary = `${target}.tmp-${process.pid}`
  let descriptor = null
  try {
    descriptor = fs.openSync(temporary, "w", PRIVATE_FILE_MODE)
    fs.writeFileSync(descriptor, JSON.stringify(value))
    fs.fsyncSync(descriptor)
    fs.closeSync(descriptor)
    descriptor = null
    fs.renameSync(temporary, target)
  } catch (error) {
    if (descriptor !== null) {
      try {
        fs.closeSync(descriptor)
      } catch {
        // 描述符可能已经关闭。
      }
    }
    try {
      fs.unlinkSync(temporary)
    } catch {
      // 临时文件可能已被替换或删除。
    }
    throw error
  }
}

function errorMessage(error) {
  if (error instanceof Error && error.message) {
    return error.message
  }
  if (typeof error === "string") {
    return error
  }
  try {
    return JSON.stringify(error)
  } catch {
    return String(error)
  }
}

function codedError(code, detail) {
  const error = new Error(detail)
  error.notifyCode = code
  return error
}

// codedErrorWithCause 保留桌面端原始错误，便于在输出通道定位失败原因。
function codedErrorWithCause(code, detail, cause) {
  const error = codedError(code, detail)
  error.causeDetail = cause
  return error
}

// redactReplyText 去掉错误信息中可能回显的回复正文，微信提示只保留失败原因。
function redactReplyText(value, text) {
  let detail = errorMessage(value).trim() || "未知错误"
  if (text) {
    detail = detail.split(text).join("[REDACTED]")
    const encoded = JSON.stringify(text)
    if (encoded.length >= 2) {
      detail = detail.split(encoded.slice(1, -1)).join("[REDACTED]")
    }
  }
  return detail.slice(0, RESULT_MAX_CHARS)
}

function removeFile(target, log = null) {
  try {
    fs.unlinkSync(target)
  } catch (error) {
    if (error && error.code !== "ENOENT") {
      const report = log || activeInstance?.log
      report?.(`remove ${target} failed: ${errorMessage(error)}`)
    }
  }
}

function readJob(pathname) {
  const parsed = JSON.parse(fs.readFileSync(pathname, "utf8"))
  if (
    !parsed ||
    typeof parsed !== "object" ||
    typeof parsed.id !== "string" ||
    typeof parsed.sessionID !== "string" ||
    typeof parsed.text !== "string" ||
    !parsed.sessionID.trim() ||
    !parsed.text.trim()
  ) {
    throw new Error("任务字段不完整")
  }
  if (!SESSION_ID_PATTERN.test(parsed.sessionID.trim())) {
    throw new Error("会话标识非法")
  }
  if (parsed.text.length > MAX_REPLY_TEXT_CHARS) {
    throw new Error("回复正文非法或过长")
  }
  // V2 只接受适配器解析好的显式 Cascade 标识；缺失或非法直接判任务无效，
  // 绝不回退到 sessionID 或最近会话。
  if (typeof parsed.targetID !== "string" || !parsed.targetID.trim()) {
    throw new Error("缺少精确会话标识")
  }
  if (!SESSION_ID_PATTERN.test(parsed.targetID.trim())) {
    throw new Error("精确会话标识非法")
  }
  return { ...parsed, sessionID: parsed.sessionID.trim(), targetID: parsed.targetID.trim() }
}

function jobExpired(job, now = Date.now()) {
  const expiresAt = Date.parse(job.expiresAt)
  return Number.isFinite(expiresAt) && expiresAt > 0 && expiresAt <= now
}

// encodeVarint 写出 protobuf 的 base-128 变长整数。
function encodeVarint(value) {
  const bytes = []
  let remaining = value
  while (remaining > 0x7f) {
    bytes.push((remaining & 0x7f) | 0x80)
    remaining = Math.floor(remaining / 0x80)
  }
  bytes.push(remaining)
  return Buffer.from(bytes)
}

function encodeLengthDelimited(fieldNumber, payload) {
  return Buffer.concat([
    encodeVarint((fieldNumber << 3) | 0x02),
    encodeVarint(payload.length),
    payload,
  ])
}

// encodeCascadeInput 按桌面端 proto 结构写出聊天输入：
// SendCascadeInputRequest{ items: [TextOrScopeItem{ text }] }。
function encodeCascadeInput(text) {
  const item = encodeLengthDelimited(1, Buffer.from(text, "utf8"))
  return encodeLengthDelimited(1, item)
}

// encodeChatAction 生成 SendActionToChatPanelRequest 的 JSON 表示，payload 为 bytes 字段。
function encodeChatAction(actionType, payload) {
  return JSON.stringify({ actionType, payload: [payload.toString("base64")] })
}

// createSpoolStore 只负责本地收件箱：目录、任务认领、结果与心跳文件。
function createSpoolStore(directory, log) {
  const pendingDir = path.join(directory, "pending")
  const processingDir = path.join(directory, "processing")
  const resultDir = path.join(directory, "results")
  const heartbeatDir = path.join(directory, "heartbeats")

  function ensureDirectories() {
    for (const dir of [directory, pendingDir, processingDir, resultDir, heartbeatDir]) {
      ensurePrivateDirectory(dir)
    }
  }

  function writeResult(jobID, ok, code = "", error = "") {
    if (!JOB_ID_PATTERN.test(jobID)) {
      return
    }
    try {
      writeJSONAtomic(path.join(resultDir, `${jobID}.json`), { ok, code, error })
    } catch (writeError) {
      log(`write result failed job=${jobID}: ${errorMessage(writeError)}`)
    }
  }

  function writeHeartbeat(instanceID, ready) {
    try {
      writeJSONAtomic(path.join(heartbeatDir, `${instanceID}.json`), {
        ready,
        timestamp: new Date().toISOString(),
      })
    } catch (error) {
      log(`write heartbeat failed: ${errorMessage(error)}`)
    }
  }

  function removeHeartbeat(instanceID) {
    removeFile(path.join(heartbeatDir, `${instanceID}.json`), log)
  }

  // processing 文件表示扩展已经认领任务；崩溃后重放可能重复回复，因此只报错不重试。
  function recoverProcessing(now) {
    let names = []
    try {
      names = fs.readdirSync(processingDir).filter((name) => name.endsWith(".json"))
    } catch (error) {
      log(`read processing queue failed: ${errorMessage(error)}`)
      return
    }

    for (const name of names) {
      const jobID = name.slice(0, -5)
      const processingPath = path.join(processingDir, name)
      if (fs.existsSync(path.join(resultDir, name))) {
        removeFile(processingPath, log)
        continue
      }

      try {
        const job = readJob(processingPath)
        if (jobExpired(job, now)) {
          writeResult(jobID, false, CODE_TURN_FAILED, "回复任务已过期且未确认，未自动重试")
          removeFile(processingPath, log)
          continue
        }
        const modifiedAt = fs.statSync(processingPath).mtimeMs
        if (now - modifiedAt < PROCESSING_STALE_MS) {
          continue
        }
        writeResult(
          jobID,
          false,
          CODE_TURN_FAILED,
          "引用回复处理中断，未自动重试以避免重复执行",
        )
        removeFile(processingPath, log)
      } catch (error) {
        writeResult(
          jobID,
          false,
          CODE_INVALID_JOB,
          `引用回复任务无效：${redactReplyText(error, "")}`,
        )
        removeFile(processingPath, log)
      }
    }
  }

  function listPendingJobIDs() {
    let names = []
    try {
      names = fs.readdirSync(pendingDir).filter((name) => name.endsWith(".json"))
    } catch (error) {
      log(`read pending queue failed: ${errorMessage(error)}`)
      return []
    }
    return names
      .map((name) => name.slice(0, -5))
      .filter((jobID) => JOB_ID_PATTERN.test(jobID))
  }

  // claim 把任务从 pending 原子移动到 processing，返回 null 表示本轮不处理。
  function claim(jobID) {
    const name = `${jobID}.json`
    const pendingPath = path.join(pendingDir, name)
    const processingPath = path.join(processingDir, name)

    let job = null
    try {
      job = readJob(pendingPath)
      if (job.id !== jobID) {
        throw new Error("任务标识与文件名不一致")
      }
    } catch (error) {
      const detail = redactReplyText(error, "")
      log(`rejected job=${jobID}: ${detail}`)
      writeResult(jobID, false, CODE_INVALID_JOB, `引用回复任务无效：${detail}`)
      removeFile(pendingPath, log)
      return null
    }

    if (jobExpired(job)) {
      log(`expired job=${jobID}`)
      writeResult(jobID, false, CODE_TURN_FAILED, "会话 prompt 任务已过期且未确认，未自动重试")
      removeFile(pendingPath, log)
      return null
    }

    try {
      fs.renameSync(pendingPath, processingPath)
    } catch (error) {
      if (!error || error.code !== "ENOENT") {
        log(`claim job failed job=${jobID}: ${errorMessage(error)}`)
      }
      return null
    }
    return { job, processingPath }
  }

  return {
    ensureDirectories,
    writeResult,
    writeHeartbeat,
    removeHeartbeat,
    recoverProcessing,
    listPendingJobIDs,
    claim,
  }
}

function classifyCommandFailure(error) {
  const detail = errorMessage(error).trim() || "未知错误"
  if (/invalid_message|invalid_session_id|invalid_message_target/i.test(detail)) {
    return codedErrorWithCause(CODE_INVALID_JOB, "回复任务无效，请重新引用原通知后再试", detail)
  }
  if (/not[_ -]?found|does not exist|unknown cascade|no such cascade/i.test(detail)) {
    return codedErrorWithCause(
      CODE_SESSION_NOT_FOUND,
      "目标 Devin 会话不存在或已删除，请确认会话后再试",
      detail,
    )
  }
  if (/command .*not found|not registered|no command/i.test(detail)) {
    return codedErrorWithCause(CODE_DESKTOP_UNAVAILABLE, DESKTOP_UNAVAILABLE_MESSAGE, detail)
  }
  return codedErrorWithCause(CODE_TURN_FAILED, detail, detail)
}

// mapACPChannelFailure 把 ACP 通道错误转成微信可读结果：通道缺失属于桌面端
// 能力问题，写入失败只影响本次回复，两者都不改投新会话。
function mapACPChannelFailure(error) {
  if (!error || !error.acpCode) {
    return classifyCommandFailure(error)
  }
  const cause = error.causeDetail || errorMessage(error)
  if (error.acpCode === ACP_ERROR_CODES.WRITE_FAILED) {
    return codedErrorWithCause(CODE_TURN_FAILED, error.message, cause)
  }
  return codedErrorWithCause(CODE_DESKTOP_UNAVAILABLE, error.message, cause)
}

// createCascadeBridge 只负责把回复交给 Devin 桌面端，并按会话类型选择通道。
function createCascadeBridge({
  getCommands,
  executeCommand,
  openPanel,
  acpChannel,
  settleDelayMs = SESSION_OPEN_SETTLE_MS,
  log,
}) {
  let hasDirectCommand = false
  let hasChatActionCommand = false
  let hasOpenACPSessionCommand = false

  async function refreshAvailability() {
    try {
      const commands = await getCommands()
      if (!Array.isArray(commands)) {
        hasDirectCommand = false
        hasChatActionCommand = false
        hasOpenACPSessionCommand = false
      } else {
        hasDirectCommand = commands.includes(DIRECT_SEND_COMMAND)
        hasChatActionCommand = commands.includes(CHAT_ACTION_COMMAND)
        hasOpenACPSessionCommand = commands.includes(OPEN_ACP_SESSION_COMMAND)
      }
    } catch (error) {
      hasDirectCommand = false
      hasChatActionCommand = false
      hasOpenACPSessionCommand = false
      log(`probe Devin reply commands failed: ${errorMessage(error)}`)
    }
    return isAvailable()
  }

  function isAvailable() {
    return isACPChannelAvailable() || hasDirectCommand || hasChatActionCommand
  }

  // ACP 会话走桌面端常驻的 ACP 通道，和桌面端命令是否注册无关。
  function isACPChannelAvailable() {
    if (!acpChannel || typeof acpChannel.isAvailable !== "function") {
      return false
    }
    try {
      return acpChannel.isAvailable() === true
    } catch (error) {
      log(`probe Devin ACP channel failed: ${errorMessage(error)}`)
      return false
    }
  }

  async function dispatchAction(actionType, payload) {
    try {
      await executeCommand(CHAT_ACTION_COMMAND, encodeChatAction(actionType, payload))
    } catch (error) {
      throw classifyCommandFailure(error)
    }
  }

  async function sendDirect(targetID, text) {
    try {
      await executeCommand(DIRECT_SEND_COMMAND, { cascadeId: targetID, text })
    } catch (error) {
      throw classifyCommandFailure(error)
    }
  }

  // openLegacyTargetSession 切换旧 Cascade，供精确直发命令建立活动运行态。
  async function openLegacyTargetSession(targetID) {
    if (typeof openPanel === "function") {
      try {
        await openPanel()
      } catch (error) {
        // 面板打开失败不影响直发：桌面端仍可能保留活动聊天客户端。
        log(`open cascade panel failed: ${errorMessage(error)}`)
      }
    }
    await dispatchAction(OPEN_LEGACY_SESSION_ACTION, Buffer.from(targetID, "utf8"))
    if (settleDelayMs > 0) {
      await new Promise((resolve) => setTimeout(resolve, settleDelayMs))
    }
  }

  // 精确命令只认活动运行态；未激活会话需要先切过去加载。
  function isInactiveSessionFailure(error) {
    const detail = `${error.notifyCode || ""} ${error.causeDetail || ""} ${error.message || ""}`
    return /session_not_found|run state not found|not in active sessions/i.test(detail)
  }

  async function sendThroughChatPanel(targetID, text) {
    if (typeof openPanel === "function") {
      try {
        await openPanel()
      } catch (error) {
        // 面板打开失败不影响后续动作：桌面端会自动选择可用的聊天客户端。
        log(`open cascade panel failed: ${errorMessage(error)}`)
      }
    }
    await dispatchAction(OPEN_LEGACY_SESSION_ACTION, Buffer.from(targetID, "utf8"))
    if (settleDelayMs > 0) {
      await new Promise((resolve) => setTimeout(resolve, settleDelayMs))
    }
    await dispatchAction(SEND_INPUT_ACTION, encodeCascadeInput(text))
  }

  // activateACPSession 只负责把会话切到前台，失败不影响正文递送。
  async function activateACPSession(targetID) {
    if (!hasOpenACPSessionCommand) {
      return
    }
    try {
      await executeCommand(OPEN_ACP_SESSION_COMMAND, { sessionId: targetID })
    } catch (error) {
      log(`activate acp session failed cascade=${targetID}: ${errorMessage(error)}`)
      return
    }
    if (settleDelayMs > 0) {
      await new Promise((resolve) => setTimeout(resolve, settleDelayMs))
    }
  }

  // sendThroughACPSession 直接把正文写进 ACP 子进程，写入成功即已进入原会话。
  async function sendThroughACPSession(targetID, sessionID, text) {
    if (!acpChannel || typeof acpChannel.send !== "function") {
      throw codedError(CODE_DESKTOP_UNAVAILABLE, ACP_CHANNEL_MISSING_MESSAGE)
    }
    await activateACPSession(targetID)
    try {
      await acpChannel.send({ sessionId: resolveACPSessionID(sessionID), text })
    } catch (error) {
      throw mapACPChannelFailure(error)
    }
  }

  async function send(job) {
    if (!isAvailable()) {
      throw codedError(CODE_DESKTOP_UNAVAILABLE, DESKTOP_UNAVAILABLE_MESSAGE)
    }

    // V2 只用适配器解析好的显式 Cascade 标识，不再回退到 sessionID。
    const targetID = job.targetID
    if (!targetID) {
      throw codedError(CODE_INVALID_JOB, MISSING_TARGET_MESSAGE)
    }
    if (isACPSession(targetID)) {
      log(`activating acp session=${job.sessionID} cascade=${targetID}`)
      await sendThroughACPSession(targetID, job.sessionID, job.text)
      log(`reply delivered channel=acp-stdio session=${job.sessionID} cascade=${targetID}`)
      return
    }

    if (!hasDirectCommand) {
      await sendThroughChatPanel(targetID, job.text)
      log(`reply delivered channel=chat-action session=${job.sessionID} cascade=${targetID}`)
      return
    }

    try {
      await sendDirect(targetID, job.text)
      log(`reply delivered channel=direct session=${job.sessionID} cascade=${targetID}`)
      return
    } catch (error) {
      if (!isInactiveSessionFailure(error)) {
        throw error
      }
      log(`activating legacy session=${job.sessionID} cascade=${targetID}`)
    }

    await openLegacyTargetSession(targetID)
    await sendDirect(targetID, job.text)
    log(`reply delivered channel=direct-after-activate session=${job.sessionID} cascade=${targetID}`)
  }

  return { refreshAvailability, isAvailable, send }
}

function newInstanceID() {
  return `${process.pid}-${Date.now().toString(36)}-${Math.random()
    .toString(36)
    .slice(2, 10)}`
}

function activate(context, overrides) {
  if (!REPLY_DIR) {
    return
  }

  const runtime = {
    getCommands: () => vscode.commands.getCommands(true),
    executeCommand: (command, argument) => vscode.commands.executeCommand(command, argument),
    openCascadePanel: () => {
      const cascade = vscode.Cascade
      if (cascade && typeof cascade.openPanel === "function") {
        return cascade.openPanel()
      }
      return undefined
    },
    sessionOpenSettleMs: SESSION_OPEN_SETTLE_MS,
    getActiveHandles: () => process._getActiveHandles(),
    acpWriteTimeoutMs: DEFAULT_WRITE_TIMEOUT_MS,
    heartbeatIntervalMs: HEARTBEAT_INTERVAL_MS,
    pumpIntervalMs: PUMP_INTERVAL_MS,
    ...(overrides || {}),
  }

  const output = vscode.window.createOutputChannel("Agent-notify Devin Reply")
  context.subscriptions.push(output)
  const log = (message) => output.appendLine(`[agent-notify] ${message}`)

  const store = createSpoolStore(REPLY_DIR, log)
  const acpChannel = createACPStdioBridge({
    getHandles: runtime.getActiveHandles,
    createRequestId: runtime.createACPRequestId,
    writeTimeoutMs: runtime.acpWriteTimeoutMs,
    log,
  })
  const bridge = createCascadeBridge({
    getCommands: runtime.getCommands,
    executeCommand: runtime.executeCommand,
    openPanel: runtime.openCascadePanel,
    acpChannel,
    settleDelayMs: runtime.sessionOpenSettleMs,
    log,
  })
  store.ensureDirectories()

  const instanceID = newInstanceID()
  let disposed = false
  let pumping = false

  async function refreshState() {
    await bridge.refreshAvailability()
    if (!disposed) {
      store.writeHeartbeat(instanceID, bridge.isAvailable())
    }
  }

  async function pumpOnce() {
    if (pumping || disposed) {
      return
    }
    pumping = true
    try {
      store.recoverProcessing(Date.now())
      if (!bridge.isAvailable()) {
        return
      }
      for (const jobID of store.listPendingJobIDs()) {
        if (disposed) {
          return
        }
        const claimed = store.claim(jobID)
        if (!claimed) {
          continue
        }
        try {
          await bridge.send(claimed.job)
          store.writeResult(claimed.job.id, true)
        } catch (error) {
          const code = error.notifyCode || CODE_TURN_FAILED
          const detail = redactReplyText(error, claimed.job.text)
          const cause = redactReplyText(error.causeDetail || "", claimed.job.text)
          const causeSuffix = cause && cause !== detail ? ` cause=${cause}` : ""
          log(
            `reply failed session=${claimed.job.sessionID} cascade=${claimed.job.targetID} code=${code} detail=${detail}${causeSuffix}`,
          )
          store.writeResult(claimed.job.id, false, code, detail)
        } finally {
          removeFile(claimed.processingPath, log)
        }
      }
    } finally {
      pumping = false
    }
  }

  const stateTimer = setInterval(() => {
    void refreshState()
  }, runtime.heartbeatIntervalMs)
  const pumpTimer = setInterval(() => {
    void pumpOnce()
  }, runtime.pumpIntervalMs)

  const instance = {
    dispose() {
      disposed = true
      clearInterval(stateTimer)
      clearInterval(pumpTimer)
      store.removeHeartbeat(instanceID)
      if (activeInstance === instance) {
        activeInstance = null
      }
    },
  }
  activeInstance = instance
  context.subscriptions.push(instance)

  void refreshState().then(() => pumpOnce())
}

function deactivate() {
  activeInstance?.dispose()
}

module.exports = { activate, deactivate }
