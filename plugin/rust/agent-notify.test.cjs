const assert = require("node:assert/strict")
const fs = require("node:fs")
const os = require("node:os")
const path = require("node:path")
const test = require("node:test")
const { pathToFileURL } = require("node:url")

const replyDir = fs.mkdtempSync(path.join(os.tmpdir(), "agentnotify-opencode-plugin-"))
process.env.AGENT_NOTIFY_OPENCODE_REPLY_DIR = replyDir
process.env.AGENT_NOTIFY_OPENCODE_REPLY_TIMEOUT_MS = "120"
process.env.AGENT_NOTIFY_INGRESS_BIN = path.join(replyDir, "missing-ingress.exe")
delete process.env.AGENT_NOTIFY_OFF

const pluginModule = import(
  pathToFileURL(path.join(__dirname, "agent-notify.ts")).href,
)

function fakeContext(promptImpl) {
  return {
    session: {
      get: async () => ({ data: { title: "插件测试" } }),
      context: async () => ({
        data: [
          {
            info: { role: "assistant" },
            parts: [{ type: "text", text: "任务已完成" }],
          },
        ],
      }),
      prompt: promptImpl,
    },
    event: {
      subscribe: async function* subscribe() {
        yield* []
      },
    },
  }
}

function writeJob(id, text, overrides = {}) {
  const pending = path.join(replyDir, "pending")
  fs.mkdirSync(pending, { recursive: true })
  fs.writeFileSync(
    path.join(pending, `${id}.json`),
    JSON.stringify({
      id,
      sessionID: "session-1",
      text,
      createdAt: new Date().toISOString(),
      expiresAt: new Date(Date.now() + 60_000).toISOString(),
      ...overrides,
    }),
  )
}

async function waitFor(predicate, timeoutMs = 2_000) {
  const deadline = Date.now() + timeoutMs
  while (Date.now() < deadline) {
    if (predicate()) {
      return
    }
    await new Promise((resolve) => setTimeout(resolve, 10))
  }
  throw new Error("waitFor timeout")
}

function readResult(id) {
  return JSON.parse(
    fs.readFileSync(path.join(replyDir, "results", `${id}.json`), "utf8"),
  )
}

test.after(() => {
  fs.rmSync(replyDir, { recursive: true, force: true })
})

test("completion event has a stable idempotency key", async () => {
  const { __test } = await pluginModule
  const event = { id: "event-9", type: "session.execution.succeeded" }
  const first = __test.completionEnvelope("session-1", "标题", "正文", event)
  const second = __test.completionEnvelope("session-1", "标题", "正文", event)

  assert.equal(first.protocolVersion, 1)
  assert.equal(first.kind, "agent.event")
  assert.equal(first.agentId, "opencode")
  assert.equal(first.payload.eventType, "session.completed")
  assert.equal(
    first.payload.idempotencyKey,
    "opencode:session-1:event-9",
  )
  assert.equal(
    second.payload.idempotencyKey,
    first.payload.idempotencyKey,
  )
})

test("heartbeat, at-most-once claim, timeout and dispose are bounded", async () => {
  const { __test, default: plugin } = await pluginModule
  const promptCalls = []
  const ctx = fakeContext(async (input) => {
    promptCalls.push(input)
  })

  const dispose = await plugin.setup(ctx)
  await waitFor(() =>
    fs.existsSync(path.join(replyDir, "heartbeats")),
  )
  writeJob("job-success", "继续处理")
  await __test.processReplyJobs(ctx, "test-instance")
  assert.deepEqual(readResult("job-success"), { ok: true, error: "" })
  assert.equal(promptCalls.length, 1)

  writeJob("job-success", "继续处理")
  await __test.processReplyJobs(ctx, "test-instance")
  assert.equal(promptCalls.length, 1)

  const never = new Promise(() => {})
  const timeoutCtx = fakeContext(() => never)
  writeJob("job-timeout", "继续处理")
  await __test.processReplyJobs(timeoutCtx, "test-instance")
  const timeoutResult = readResult("job-timeout")
  assert.equal(timeoutResult.ok, false)
  assert.match(timeoutResult.error, /超时|未自动重试/)

  const heartbeatDir = path.join(replyDir, "heartbeats")
  const heartbeats = fs.readdirSync(heartbeatDir)
  assert.ok(heartbeats.length > 0)
  await dispose()
  assert.equal(fs.readdirSync(heartbeatDir).length, 0)
})

test("ingress failure is swallowed and does not reject OpenCode", async () => {
  const { __test } = await pluginModule
  const ctx = fakeContext(async () => {})
  await assert.doesNotReject(() =>
    __test.dispatchCompletion(ctx, "session-1", { id: "event-10" }),
  )
})
