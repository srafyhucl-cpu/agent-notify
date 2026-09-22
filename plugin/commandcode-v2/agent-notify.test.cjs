const assert = require("node:assert/strict")
const fs = require("node:fs")
const os = require("node:os")
const path = require("node:path")
const test = require("node:test")
const { pathToFileURL } = require("node:url")

// 全部指向临时沙箱：测试绝不读写真实 %USERPROFILE%\.commandcode 或 %LOCALAPPDATA%。
const sandbox = fs.mkdtempSync(
  path.join(os.tmpdir(), "agentnotify-commandcode-plugin-"),
)
const replyDir = path.join(sandbox, "commandcode-reply-inbox")
const configFile = path.join(sandbox, "config.json")
const markerFile = path.join(sandbox, "commandcode.off")

process.env.AGENT_NOTIFY_COMMANDCODE_REPLY_DIR = replyDir
process.env.AGENT_NOTIFY_COMMANDCODE_MARKER_FILE = markerFile
process.env.AGENT_NOTIFY_CONFIG_FILE = configFile
process.env.AGENT_NOTIFY_INGRESS_BIN = path.join(sandbox, "missing-ingress.exe")
process.env.AGENT_NOTIFY_TEMP_DIR = path.join(sandbox, "temp")
process.env.USERPROFILE = sandbox
process.env.HOME = sandbox
delete process.env.AGENT_NOTIFY_OFF
delete process.env.AGENT_NOTIFY_DEBUG

const modModule = import(
  pathToFileURL(path.join(__dirname, "agent-notify.ts")).href,
)

const PENDING_DIR = path.join(replyDir, "pending")
const PROCESSING_DIR = path.join(replyDir, "processing")
const RESULT_DIR = path.join(replyDir, "results")
const HEARTBEAT_DIR = path.join(replyDir, "heartbeats")
const WINDOW_FILE = path.join(replyDir, "window.json")
const DEBUG_LOG_FILE = path.join(process.env.AGENT_NOTIFY_TEMP_DIR, "commandcode-debug.log")

function writeConfig(value) {
  fs.writeFileSync(configFile, JSON.stringify(value))
}

function writeInboxWindow(value) {
  fs.mkdirSync(replyDir, { recursive: true })
  fs.writeFileSync(WINDOW_FILE, JSON.stringify(value))
}

function writeJob(id, overrides = {}) {
  fs.mkdirSync(PENDING_DIR, { recursive: true })
  fs.writeFileSync(
    path.join(PENDING_DIR, `${id}.json`),
    JSON.stringify({
      id,
      sessionID: "session-1",
      text: "继续处理",
      createdAt: new Date().toISOString(),
      expiresAt: new Date(Date.now() + 60_000).toISOString(),
      ...overrides,
    }),
  )
}

function readResult(id) {
  return JSON.parse(
    fs.readFileSync(path.join(RESULT_DIR, `${id}.json`), "utf8"),
  )
}

function directoryNames(directory) {
  try {
    return fs.readdirSync(directory).sort()
  } catch {
    return []
  }
}

/** 每个回复泵用例都从空收件箱开始，避免互相看见对方留下的任务。 */
function resetInbox() {
  for (const directory of [PENDING_DIR, PROCESSING_DIR, RESULT_DIR]) {
    fs.rmSync(directory, { recursive: true, force: true })
  }
}

test.before(async () => {
  const { __test } = await modModule
  await __test.loadFs()
})

test.after(() => {
  fs.rmSync(sandbox, { recursive: true, force: true })
})

test("run_end envelope uses the ingress protocol fields", async () => {
  const { __test } = await modModule
  const payload = __test.buildRunEndPayload("session-1", "会话标题", "任务结果")
  const envelope = __test.buildRunEndEnvelope(payload)

  assert.equal(envelope.protocolVersion, 1)
  assert.equal(envelope.kind, "agent.event")
  assert.equal(envelope.agentId, "commandcode")
  assert.match(envelope.requestId, /^[0-9a-f-]{36}$/)
  assert.equal(typeof envelope.payload, "object")
  assert.equal(envelope.payload.eventType, "run_end")
  assert.equal(envelope.payload.sessionId, "session-1")
  assert.equal(envelope.payload.title, "会话标题")
  assert.equal(envelope.payload.body, "任务结果")
})

test("run_end payload omits the title and defaults an empty body", async () => {
  const { __test } = await modModule
  const payload = __test.buildRunEndPayload("session-1", "   ", "   ")

  assert.equal("title" in payload, false)
  assert.equal(payload.body, "任务已完成。")
})

test("run_end body stays below the ingress body limit", async () => {
  const { __test } = await modModule
  const oversized = "字".repeat(100_000)
  const truncated = __test.truncateBody(oversized)

  assert.ok(Buffer.byteLength(truncated, "utf8") < 64 * 1024)
  assert.match(truncated, /已截断）$/)
  assert.equal(__test.truncateBody("短正文"), "短正文")
})

test("reply window is closed by default and follows config, marker and env", async () => {
  const { __test } = await modModule
  fs.rmSync(configFile, { force: true })
  fs.rmSync(markerFile, { force: true })
  fs.rmSync(WINDOW_FILE, { force: true })
  delete process.env.AGENT_NOTIFY_COMMANDCODE_WINDOW_SEC

  assert.equal(__test.windowSeconds(), 0, "默认必须是 0（关闭）")
  assert.equal(__test.replyWindowEnabled(), false)

  writeConfig({ commandCodeReplyWindowSec: 5 })
  assert.equal(__test.windowSeconds(), 5)
  assert.equal(__test.replyWindowEnabled(), true)

  fs.writeFileSync(markerFile, "")
  assert.equal(__test.replyWindowEnabled(), false, "marker 存在时必须停")
  fs.rmSync(markerFile, { force: true })

  process.env.AGENT_NOTIFY_COMMANDCODE_WINDOW_SEC = "9999"
  assert.equal(__test.windowSeconds(), 600, "窗口必须收敛到上限")
  process.env.AGENT_NOTIFY_COMMANDCODE_WINDOW_SEC = "0"
  assert.equal(
    __test.windowSeconds(),
    5,
    "环境变量只有大于 0 才覆盖配置（与 Go 版一致）",
  )
  delete process.env.AGENT_NOTIFY_COMMANDCODE_WINDOW_SEC
})

test("reply window prefers the inbox window file over the legacy config", async () => {
  const { __test } = await modModule
  fs.rmSync(WINDOW_FILE, { force: true })
  writeConfig({ commandCodeReplyWindowSec: 5 })

  assert.equal(__test.windowSeconds(), 5, "没有 window.json 时回退旧配置")

  writeInboxWindow({ commandCodeReplyWindowSec: 90 })
  assert.equal(__test.windowSeconds(), 90, "应用写的 window.json 必须优先于旧配置")

  writeInboxWindow({ commandCodeReplyWindowSec: 0 })
  assert.equal(
    __test.windowSeconds(),
    0,
    "界面显式关闭（0）必须盖过旧配置，否则 mod 会开出适配器不认的窗口",
  )
  assert.equal(__test.replyWindowEnabled(), false)

  writeInboxWindow({ commandCodeReplyWindowSec: 700 })
  assert.equal(__test.windowSeconds(), 600, "window.json 同样收敛到上限")

  fs.rmSync(WINDOW_FILE, { force: true })
  assert.equal(__test.windowSeconds(), 5, "文件被删除后必须回退旧配置")
})

test("environment overrides the inbox window file", async () => {
  const { __test } = await modModule
  writeInboxWindow({ commandCodeReplyWindowSec: 90 })

  process.env.AGENT_NOTIFY_COMMANDCODE_WINDOW_SEC = "120"
  assert.equal(__test.windowSeconds(), 120, "环境变量显式覆盖 window.json")

  process.env.AGENT_NOTIFY_COMMANDCODE_WINDOW_SEC = "0"
  assert.equal(
    __test.windowSeconds(),
    90,
    "环境变量只有大于 0 才覆盖（与既有语义一致）",
  )

  process.env.AGENT_NOTIFY_COMMANDCODE_WINDOW_SEC = "9999"
  assert.equal(__test.windowSeconds(), 600, "环境变量上限仍是 600")

  delete process.env.AGENT_NOTIFY_COMMANDCODE_WINDOW_SEC
  fs.rmSync(WINDOW_FILE, { force: true })
})

test("invalid inbox window file falls back to the legacy config and logs why", async () => {
  const { __test } = await modModule
  writeConfig({ commandCodeReplyWindowSec: 5 })
  process.env.AGENT_NOTIFY_DEBUG = "1"
  fs.rmSync(DEBUG_LOG_FILE, { force: true })

  fs.mkdirSync(replyDir, { recursive: true })
  fs.writeFileSync(WINDOW_FILE, "{ 不是合法 JSON")
  assert.equal(__test.windowSeconds(), 5, "非法 JSON 必须回退旧配置且不抛出")

  fs.writeFileSync(WINDOW_FILE, JSON.stringify({ commandCodeReplyWindowSec: "300" }))
  assert.equal(__test.windowSeconds(), 5, "非数字值必须回退旧配置")

  fs.writeFileSync(WINDOW_FILE, JSON.stringify({ other: 1 }))
  assert.equal(__test.windowSeconds(), 5, "缺少窗口键必须回退旧配置")

  const log = fs.readFileSync(DEBUG_LOG_FILE, "utf8")
  assert.match(log, /window file invalid json/, "非法 JSON 的原因必须写进调试日志")
  assert.match(log, /window file invalid value/, "非法值的原因必须写进调试日志")

  delete process.env.AGENT_NOTIFY_DEBUG
  fs.rmSync(WINDOW_FILE, { force: true })
  fs.rmSync(DEBUG_LOG_FILE, { force: true })
})

test("heartbeat reports the exact session and window state", async () => {
  const { __test } = await modModule
  const state = __test.createInstanceState()
  state.sessionId = "session-1"
  state.windowOpen = true
  __test.writeHeartbeat(state)

  const heartbeat = JSON.parse(
    fs.readFileSync(path.join(HEARTBEAT_DIR, `${state.instanceID}.json`), "utf8"),
  )
  assert.equal(heartbeat.ready, true)
  assert.equal(heartbeat.sessionId, "session-1")
  assert.equal(heartbeat.windowOpen, true)
  assert.match(heartbeat.timestamp, /^\d{4}-\d{2}-\d{2}T/)
})

test("pending job for another session is left for its owner", async () => {
  const { __test } = await modModule
  resetInbox()
  const state = __test.createInstanceState()
  state.sessionId = "session-1"
  state.windowOpen = true
  writeJob("job-other", { sessionID: "session-2" })

  __test.pumpReplyJobs(state)

  assert.deepEqual(directoryNames(PENDING_DIR), ["job-other.json"])
  assert.deepEqual(directoryNames(RESULT_DIR), [])
  assert.equal(state.injectedCount, 0)
})

test("claimed job is reported as window_closed once the window shut", async () => {
  const { __test } = await modModule
  resetInbox()
  const state = __test.createInstanceState()
  state.sessionId = "session-1"
  state.windowOpen = false
  writeJob("job-closed")

  __test.pumpReplyJobs(state)

  assert.deepEqual(directoryNames(PENDING_DIR), [])
  assert.deepEqual(directoryNames(PROCESSING_DIR), [])
  assert.equal(readResult("job-closed").ok, false)
  assert.equal(readResult("job-closed").code, "window_closed")
  assert.equal(state.injectedCount, 0)
  assert.equal(state.pendingReplyText, "")
})

test("claimed job inside the window is queued for the stop hook", async () => {
  const { __test } = await modModule
  resetInbox()
  const state = __test.createInstanceState()
  state.sessionId = "session-1"
  state.windowOpen = true
  writeJob("job-open")

  __test.pumpReplyJobs(state)

  assert.deepEqual(directoryNames(PENDING_DIR), [])
  assert.deepEqual(directoryNames(PROCESSING_DIR), [])
  assert.deepEqual(readResult("job-open"), { ok: true })
  assert.equal(state.injectedCount, 1)
  assert.equal(state.pendingReplyText, "继续处理")
})

test("expired job is rejected without injection", async () => {
  const { __test } = await modModule
  resetInbox()
  const state = __test.createInstanceState()
  state.sessionId = "session-1"
  state.windowOpen = true
  writeJob("job-expired", { expiresAt: new Date(Date.now() - 1_000).toISOString() })

  __test.pumpReplyJobs(state)

  assert.deepEqual(directoryNames(PENDING_DIR), [])
  assert.equal(readResult("job-expired").code, "invalid_job")
  assert.equal(state.injectedCount, 0)
})

test("ingress failure is swallowed and never rejects Command Code", async () => {
  const { __test } = await modModule
  const state = __test.createInstanceState()
  state.sessionId = "session-1"

  await assert.doesNotReject(() =>
    __test.pushRunEnd(state, "任务结果", "session-1"),
  )
})
