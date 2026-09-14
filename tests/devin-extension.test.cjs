"use strict"

const assert = require("node:assert/strict")
const { EventEmitter } = require("node:events")
const fs = require("node:fs")
const Module = require("node:module")
const os = require("node:os")
const path = require("node:path")
const test = require("node:test")

const extensionPath = path.resolve(
  __dirname,
  "..",
  "plugin",
  "devin-extension",
  "extension.js",
)
const acpBridgePath = path.resolve(
  __dirname,
  "..",
  "plugin",
  "devin-extension",
  "acp-bridge.js",
)
const acpBridge = require(acpBridgePath)
const originalModuleLoad = Module._load
const CHAT_ACTION_COMMAND = "devin.sendChatActionMessage"
const DIRECT_SEND_COMMAND = "windsurf2plus.remote.sendMessage"
const OPEN_ACP_SESSION_COMMAND = "devin.prioritized.openAgentInSmartPane"
const OPEN_LEGACY_SESSION_ACTION = "openCascadeIdInChatPanel"
const SEND_INPUT_ACTION = "sendCascadeInput"

function delay(milliseconds) {
  return new Promise((resolve) => setTimeout(resolve, milliseconds))
}

async function waitFor(predicate, message, timeout = 2000) {
  const deadline = Date.now() + timeout
  while (Date.now() < deadline) {
    const value = predicate()
    if (value) {
      return value
    }
    await delay(10)
  }
  throw new Error(`timed out: ${message}`)
}

function withEnvironment(overrides) {
  const previous = {}
  for (const [key, value] of Object.entries(overrides)) {
    previous[key] = process.env[key]
    process.env[key] = value
  }
  return () => {
    for (const [key, value] of Object.entries(previous)) {
      if (value === undefined) {
        delete process.env[key]
      } else {
        process.env[key] = value
      }
    }
  }
}

function loadExtension(vscodeMock) {
  delete require.cache[require.resolve(extensionPath)]
  Module._load = function load(request, parent, isMain) {
    if (request === "vscode") {
      return vscodeMock
    }
    return originalModuleLoad.call(this, request, parent, isMain)
  }
  try {
    return require(extensionPath)
  } finally {
    Module._load = originalModuleLoad
  }
}

// decodeChatAction 解析桌面端命令接收的 JSON，并取出唯一的 bytes payload。
function decodeChatAction(argument) {
  const action = JSON.parse(argument)
  assert.equal(action.payload.length, 1)
  return {
    actionType: action.actionType,
    payload: Buffer.from(action.payload[0], "base64"),
  }
}

function readVarint(buffer, start) {
  let value = 0
  let shift = 0
  let offset = start
  while (offset < buffer.length) {
    const byte = buffer[offset]
    offset += 1
    value += (byte & 0x7f) * 2 ** shift
    if ((byte & 0x80) === 0) {
      return { value, offset }
    }
    shift += 7
    if (shift > 35) {
      throw new Error("invalid protobuf varint")
    }
  }
  throw new Error("truncated protobuf varint")
}

function readLengthDelimited(buffer, start, fieldNumber) {
  const tag = readVarint(buffer, start)
  assert.equal(tag.value, (fieldNumber << 3) | 0x02)
  const length = readVarint(buffer, tag.offset)
  const end = length.offset + length.value
  assert.ok(end <= buffer.length)
  return { payload: buffer.subarray(length.offset, end), offset: end }
}

// decodeCascadeInput 从 protobuf 中读回文本，验证桌面端实际收到的正文。
function decodeCascadeInput(buffer) {
  const request = readLengthDelimited(buffer, 0, 1)
  assert.equal(request.offset, buffer.length)
  const item = readLengthDelimited(request.payload, 0, 1)
  assert.equal(item.offset, request.payload.length)
  return item.payload.toString("utf8")
}

// createMock 模拟扩展宿主和 Devin 桌面端命令，记录 open + send 的完整顺序。
// ChildProcess 只是把实例构造名伪装成 Node 的 ChildProcess，供 ACP 查找逻辑识别。
const ChildProcess = class ChildProcess {
  constructor({ spawnargs, stdin, pid = 4242 }) {
    this.spawnargs = spawnargs
    this.stdin = stdin
    this.pid = pid
  }
}

function createFakeStdin(options = {}) {
  const stdin = new EventEmitter()
  stdin.writes = []
  stdin.destroyed = false
  stdin.writable = true
  stdin.write = (chunk, encoding, callback) => {
    const done = typeof encoding === "function" ? encoding : callback
    stdin.writes.push(chunk)
    if (options.failWith) {
      if (typeof done === "function") {
        done(options.failWith)
      }
      return false
    }
    if (typeof done === "function") {
      done()
    }
    return true
  }
  return stdin
}

// createFakeACPChild 生成带正常 stdin 的 Devin ACP 子进程，并记录写入内容。
function createFakeACPChild(options = {}) {
  const stdin = options.stdin || createFakeStdin()
  const executable = options.executable || "D:\\Tools\\Devin\\devin.exe"
  const spawnargs = options.spawnargs || [executable, "acp"]
  const child = new ChildProcess({ spawnargs, stdin })
  return { child, stdin }
}

function readACPPrompts(stdin) {
  return stdin.writes.map((line) => JSON.parse(line))
}

function createMock(options = {}) {
  const state = {
    commands: options.commands ? [...options.commands] : [],
    execute: options.execute,
  }
  const calls = []
  const panel = { calls: 0 }
  return {
    state,
    calls,
    panel,
    commands: {
      async getCommands() {
        return [...state.commands]
      },
      async executeCommand(name, argument) {
        calls.push({ name, argument })
        if (state.execute) {
          return state.execute(name, argument)
        }
        return undefined
      },
    },
    window: {
      createOutputChannel() {
        return {
          appendLine() {},
          dispose() {},
        }
      },
    },
    Cascade: {
      async openPanel() {
        panel.calls += 1
        if (options.openPanel) {
          return options.openPanel()
        }
        return undefined
      },
    },
  }
}

function createContext() {
  return { subscriptions: [] }
}

function disposeContext(context) {
  for (const subscription of context.subscriptions) {
    if (subscription && typeof subscription.dispose === "function") {
      subscription.dispose()
    }
  }
}

function writeJob(directory, job) {
  const pending = path.join(directory, "pending")
  fs.mkdirSync(pending, { recursive: true })
  fs.writeFileSync(path.join(pending, `${job.id}.json`), JSON.stringify(job))
}

function makeJob(id, overrides = {}) {
  return {
    id,
    sessionID: "complete-tourmaline",
    targetID: "acp/devin-cli/complete-tourmaline",
    text: "继续检查",
    createdAt: new Date().toISOString(),
    expiresAt: new Date(Date.now() + 60_000).toISOString(),
    ...overrides,
  }
}

function readResult(directory, id) {
  const resultPath = path.join(directory, "results", `${id}.json`)
  try {
    return JSON.parse(fs.readFileSync(resultPath, "utf8"))
  } catch {
    return undefined
  }
}

function readHeartbeat(directory) {
  const heartbeatDir = path.join(directory, "heartbeats")
  let names = []
  try {
    names = fs.readdirSync(heartbeatDir).filter((name) => name.endsWith(".json"))
  } catch {
    return undefined
  }
  if (names.length === 0) {
    return undefined
  }
  return JSON.parse(fs.readFileSync(path.join(heartbeatDir, names[0]), "utf8"))
}

async function withRunningExtension(vscodeMock, overrides, run) {
  const directory = fs.mkdtempSync(path.join(os.tmpdir(), "agent-notify-devin-"))
  const restore = withEnvironment({
    AGENT_NOTIFY_DEVIN_REPLY_DIR: directory,
  })
  const context = createContext()
  try {
    const extension = loadExtension(vscodeMock)
    extension.activate(context, {
      heartbeatIntervalMs: 30,
      pumpIntervalMs: 20,
      sessionOpenSettleMs: 0,
      ...overrides,
    })
    await run(directory)
  } finally {
    disposeContext(context)
    restore()
    fs.rmSync(directory, { recursive: true, force: true })
  }
}

test("writes the ACP prompt straight into the desktop ACP process", async () => {
  const mock = createMock({ commands: [] })
  const { child, stdin } = createFakeACPChild()
  await withRunningExtension(
    mock,
    { getActiveHandles: () => [child], createACPRequestId: () => 987001 },
    async (directory) => {
      const id = "a".repeat(32)
      writeJob(directory, makeJob(id))

      const result = await waitFor(() => readResult(directory, id), "reply result")
      assert.equal(result.ok, true)
      // ACP 通道不依赖桌面端命令，因此不应调用任何命令或打开面板。
      assert.equal(mock.calls.length, 0)
      assert.equal(mock.panel.calls, 0)

      const prompts = readACPPrompts(stdin)
      assert.equal(prompts.length, 1)
      assert.equal(stdin.writes[0].endsWith("\n"), true)
      assert.deepEqual(prompts[0], {
        jsonrpc: "2.0",
        id: 987001,
        method: "session/prompt",
        params: {
          sessionId: "complete-tourmaline",
          prompt: [{ type: "text", text: "继续检查" }],
        },
      })

      const heartbeat = await waitFor(() => readHeartbeat(directory), "extension heartbeat")
      assert.equal(heartbeat.ready, true)

      await waitFor(
        () => !fs.existsSync(path.join(directory, "processing", `${id}.json`)),
        "processed job cleanup",
      )
      assert.equal(fs.existsSync(path.join(directory, "pending", `${id}.json`)), false)
    },
  )
})

test("activates the ACP pane without submitting through the legacy chat action", async () => {
  const mock = createMock({
    commands: [CHAT_ACTION_COMMAND, DIRECT_SEND_COMMAND, OPEN_ACP_SESSION_COMMAND],
  })
  const { child, stdin } = createFakeACPChild()
  await withRunningExtension(mock, { getActiveHandles: () => [child] }, async (directory) => {
    const id = "9".repeat(32)
    writeJob(directory, makeJob(id))

    const result = await waitFor(() => readResult(directory, id), "reply result")
    assert.equal(result.ok, true)
    assert.equal(mock.panel.calls, 0)
    // 只允许激活会话面板，正文必须走 ACP 通道，不能再走聊天面板提交。
    assert.deepEqual(mock.calls, [
      {
        name: OPEN_ACP_SESSION_COMMAND,
        argument: { sessionId: "acp/devin-cli/complete-tourmaline" },
      },
    ])
    const prompts = readACPPrompts(stdin)
    assert.equal(prompts.length, 1)
    assert.equal(prompts[0].method, "session/prompt")
    assert.equal(prompts[0].params.sessionId, "complete-tourmaline")
    assert.equal(prompts[0].params.prompt[0].text, "继续检查")
  })
})

test("reports a missing desktop ACP channel without falling back to the chat action", async () => {
  const mock = createMock({ commands: [CHAT_ACTION_COMMAND, DIRECT_SEND_COMMAND] })
  await withRunningExtension(mock, { getActiveHandles: () => [] }, async (directory) => {
    const id = "7".repeat(32)
    writeJob(directory, makeJob(id))

    const result = await waitFor(() => readResult(directory, id), "failed result")
    assert.equal(result.ok, false)
    assert.equal(result.code, "desktop_unavailable")
    assert.match(result.error, /Devin 桌面端 ACP 通道/)
    assert.match(result.error, /不会改投到新会话/)
    // 只允许激活会话面板，不能再把正文交给聊天面板提交。
    assert.equal(
      mock.calls.every((call) => call.name === OPEN_ACP_SESSION_COMMAND),
      true,
    )
  })
})

test("activates an inactive legacy Cascade and retries through the precise channel", async () => {
  let directCalls = 0
  const mock = createMock({
    commands: [CHAT_ACTION_COMMAND, DIRECT_SEND_COMMAND],
    execute(name) {
      if (name === DIRECT_SEND_COMMAND) {
        directCalls += 1
        if (directCalls === 1) {
          throw new Error("run state not found")
        }
      }
      return undefined
    },
  })
  await withRunningExtension(mock, {}, async (directory) => {
    const id = "8".repeat(32)
    writeJob(directory, makeJob(id, { targetID: undefined }))

    const result = await waitFor(() => readResult(directory, id), "reply result")
    assert.equal(result.ok, true)
    assert.equal(mock.panel.calls, 1)
    assert.equal(directCalls, 2)
    assert.deepEqual(
      mock.calls.map((call) => call.name),
      [DIRECT_SEND_COMMAND, CHAT_ACTION_COMMAND, DIRECT_SEND_COMMAND],
    )
    const open = decodeChatAction(mock.calls[1].argument)
    assert.equal(open.actionType, OPEN_LEGACY_SESSION_ACTION)

    const direct = mock.calls[2].argument
    assert.equal(direct.cascadeId, "complete-tourmaline")
    assert.equal(direct.text, "继续检查")
  })
})

test("falls back to the local session id for legacy jobs", async () => {
  const mock = createMock({ commands: [CHAT_ACTION_COMMAND] })
  await withRunningExtension(mock, {}, async (directory) => {
    const id = "b".repeat(32)
    writeJob(directory, makeJob(id, { targetID: undefined }))

    const result = await waitFor(() => readResult(directory, id), "reply result")
    assert.equal(result.ok, true)
    const open = decodeChatAction(mock.calls[0].argument)
    assert.equal(open.payload.toString("utf8"), "complete-tourmaline")
  })
})

test("encodes long UTF-8 reply text with a valid protobuf varint", async () => {
  const text = "你好，Devin。".repeat(40)
  const mock = createMock({ commands: [CHAT_ACTION_COMMAND] })
  await withRunningExtension(mock, {}, async (directory) => {
    const id = "c".repeat(32)
    writeJob(directory, makeJob(id, { targetID: undefined, text }))

    const result = await waitFor(() => readResult(directory, id), "reply result")
    assert.equal(result.ok, true)
    const send = decodeChatAction(mock.calls[1].argument)
    assert.equal(decodeCascadeInput(send.payload), text)
  })
})

test("continues sending when opening the panel fails", async () => {
  const mock = createMock({
    commands: [CHAT_ACTION_COMMAND],
    openPanel() {
      throw new Error("panel unavailable")
    },
  })
  await withRunningExtension(mock, {}, async (directory) => {
    const id = "d".repeat(32)
    writeJob(directory, makeJob(id, { targetID: undefined }))

    const result = await waitFor(() => readResult(directory, id), "reply result")
    assert.equal(result.ok, true)
    assert.equal(mock.calls.length, 2)
    assert.equal(mock.panel.calls, 1)
  })
})

test("reports not ready until a desktop reply channel exists", async () => {
  const mock = createMock({ commands: [] })
  await withRunningExtension(mock, {}, async (directory) => {
    const id = "e".repeat(32)
    writeJob(directory, makeJob(id, { targetID: undefined }))

    const heartbeat = await waitFor(() => readHeartbeat(directory), "extension heartbeat")
    assert.equal(heartbeat.ready, false)
    await delay(80)
    assert.equal(mock.calls.length, 0)
    assert.equal(readResult(directory, id), undefined)

    mock.state.commands = [CHAT_ACTION_COMMAND]
    const result = await waitFor(() => readResult(directory, id), "delivered result")
    assert.equal(result.ok, true)
    assert.equal(mock.calls.length, 2)
  })
})

test("maps desktop command failures to an actionable session error", async () => {
  const mock = createMock({
    commands: [CHAT_ACTION_COMMAND],
    execute() {
      throw new Error("run state not found")
    },
  })
  await withRunningExtension(mock, {}, async (directory) => {
    const id = "f".repeat(32)
    writeJob(directory, makeJob(id, { targetID: undefined }))

    const result = await waitFor(() => readResult(directory, id), "failed result")
    assert.equal(result.ok, false)
    assert.equal(result.code, "session_not_found")
    assert.match(result.error, /会话不存在/)
  })
})

test("redacts reply text from desktop command failures", async () => {
  const sensitive = "private reply text"
  const mock = createMock({
    commands: [CHAT_ACTION_COMMAND],
    execute() {
      throw new Error(`failed to send: ${sensitive}`)
    },
  })
  await withRunningExtension(mock, {}, async (directory) => {
    const id = "1".repeat(32)
    writeJob(directory, makeJob(id, { targetID: undefined, text: sensitive }))

    const result = await waitFor(() => readResult(directory, id), "failed result")
    assert.equal(result.ok, false)
    assert.equal(result.code, "turn_failed")
    assert.match(result.error, /\[REDACTED\]/)
    assert.equal(result.error.includes(sensitive), false)
  })
})

test("rejects an invalid reply job without calling the desktop command", async () => {
  const mock = createMock({ commands: [CHAT_ACTION_COMMAND] })
  await withRunningExtension(mock, {}, async (directory) => {
    const id = "2".repeat(32)
    writeJob(directory, makeJob(id, { sessionID: "bad session id" }))

    const result = await waitFor(() => readResult(directory, id), "invalid job result")
    assert.equal(result.ok, false)
    assert.equal(result.code, "invalid_job")
    assert.match(result.error, /无效/)
    assert.equal(mock.calls.length, 0)
  })
})

test("does not replay a stale processing job", async () => {
  const mock = createMock({ commands: [CHAT_ACTION_COMMAND] })
  await withRunningExtension(mock, {}, async (directory) => {
    const id = "3".repeat(32)
    const processing = path.join(directory, "processing")
    fs.mkdirSync(processing, { recursive: true })
    const processingPath = path.join(processing, `${id}.json`)
    fs.writeFileSync(processingPath, JSON.stringify(makeJob(id)))
    const old = new Date(Date.now() - 120_000)
    fs.utimesSync(processingPath, old, old)

    const result = await waitFor(() => readResult(directory, id), "stale result")
    assert.equal(result.ok, false)
    assert.match(result.error, /未自动重试/)
    assert.equal(mock.calls.length, 0)
    assert.equal(fs.existsSync(processingPath), false)
  })
})

test("dispose removes only its own heartbeat", async () => {
  const mock = createMock({ commands: [CHAT_ACTION_COMMAND] })
  const directory = fs.mkdtempSync(path.join(os.tmpdir(), "agent-notify-devin-"))
  const restore = withEnvironment({
    AGENT_NOTIFY_DEVIN_REPLY_DIR: directory,
  })
  let context = createContext()
  try {
    const extension = loadExtension(mock)
    extension.activate(context, {
      heartbeatIntervalMs: 30,
      pumpIntervalMs: 20,
      sessionOpenSettleMs: 0,
    })
    const heartbeatDir = path.join(directory, "heartbeats")
    const heartbeat = await waitFor(
      () => fs.readdirSync(heartbeatDir).find((name) => name.endsWith(".json")),
      "extension heartbeat",
    )
    fs.writeFileSync(path.join(heartbeatDir, "other-instance.json"), "{}")
    disposeContext(context)
    context = undefined

    assert.equal(fs.existsSync(path.join(heartbeatDir, heartbeat)), false)
    assert.equal(fs.existsSync(path.join(heartbeatDir, "other-instance.json")), true)
  } finally {
    if (context) {
      disposeContext(context)
    }
    restore()
    fs.rmSync(directory, { recursive: true, force: true })
  }
})

test("reports ACP write failures without leaking the reply text", async () => {
  const sensitive = "private ACP reply"
  const stdin = createFakeStdin({ failWith: new Error("EPIPE") })
  const { child } = createFakeACPChild({ stdin })
  const mock = createMock({ commands: [] })
  await withRunningExtension(mock, { getActiveHandles: () => [child] }, async (directory) => {
    const id = "4".repeat(32)
    writeJob(directory, makeJob(id, { text: sensitive }))

    const result = await waitFor(() => readResult(directory, id), "failed result")
    assert.equal(result.ok, false)
    assert.equal(result.code, "turn_failed")
    assert.match(result.error, /ACP 通道写入失败/)
    assert.equal(result.error.includes(sensitive), false)
  })
})

test("finds the Devin ACP child and ignores summarizer agents", () => {
  const { child: acp } = createFakeACPChild()
  const { child: summarizer } = createFakeACPChild()
  summarizer.spawnargs = ["D:\\Tools\\Devin\\devin.exe", "acp", "--agent-type", "summarizer"]
  const unrelated = new ChildProcess({
    spawnargs: ["node.exe", "acp"],
    stdin: createFakeStdin(),
  })

  assert.equal(acpBridge.findACPHandle([summarizer, unrelated, acp]), acp)
  assert.equal(acpBridge.isDevinACPHandle(summarizer), false)
})

test("reports missing and ambiguous ACP channels explicitly", () => {
  assert.throws(
    () => acpBridge.findACPHandle([]),
    (error) => error.acpCode === acpBridge.ACP_ERROR_CODES.CHANNEL_MISSING,
  )
  const { child: first } = createFakeACPChild()
  const { child: second } = createFakeACPChild()
  assert.throws(
    () => acpBridge.findACPHandle([first, second]),
    (error) => error.acpCode === acpBridge.ACP_ERROR_CODES.CHANNEL_AMBIGUOUS,
  )
})

test("treats unavailable handle enumeration as a missing ACP channel", async () => {
  const channel = acpBridge.createACPStdioBridge({ getHandles: undefined })
  assert.equal(channel.isAvailable(), false)
  await assert.rejects(
    channel.send({ sessionId: "session-1", text: "继续" }),
    (error) => error.acpCode === acpBridge.ACP_ERROR_CODES.CHANNEL_MISSING,
  )
})

test("builds a newline-terminated ACP session prompt", () => {
  const line = acpBridge.buildPromptLine({ id: 7, sessionId: "session-1", text: "继续" })
  assert.equal(line.endsWith("\n"), true)
  assert.equal(line.indexOf("\n"), line.length - 1)
  assert.deepEqual(JSON.parse(line), {
    jsonrpc: "2.0",
    id: 7,
    method: "session/prompt",
    params: { sessionId: "session-1", prompt: [{ type: "text", text: "继续" }] },
  })
})

test("rejects ACP writes when the stdin is closed or errors", async () => {
  const closed = createFakeStdin()
  closed.destroyed = true
  const { child: closedChild } = createFakeACPChild({ stdin: closed })
  const line = acpBridge.buildPromptLine({ id: 1, sessionId: "session-1", text: "继续" })
  await assert.rejects(
    acpBridge.writePromptLine(closedChild, line, { timeoutMs: 50 }),
    (error) => error.acpCode === acpBridge.ACP_ERROR_CODES.WRITE_FAILED,
  )

  const broken = createFakeStdin()
  broken.write = () => {
    process.nextTick(() => broken.emit("error", new Error("EPIPE")))
    return false
  }
  const { child: brokenChild } = createFakeACPChild({ stdin: broken })
  await assert.rejects(
    acpBridge.writePromptLine(brokenChild, line, { timeoutMs: 50 }),
    (error) =>
      error.acpCode === acpBridge.ACP_ERROR_CODES.WRITE_FAILED &&
      error.causeDetail === "EPIPE",
  )
})
