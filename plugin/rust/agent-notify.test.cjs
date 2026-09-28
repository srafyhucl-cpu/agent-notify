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

function fakeContext(promptImpl, contextImpl) {
  return {
    session: {
      get: async () => ({ data: { title: "插件测试" } }),
      context:
        contextImpl ||
        (async () => ({
          data: [
            {
              info: { role: "assistant" },
              parts: [{ type: "text", text: "任务已完成" }],
            },
          ],
        })),
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

test("session.idle submits once per assistant message", async () => {
  const { __test } = await pluginModule
  __test.resetTerminalStateForTests()
  const submitted = []
  const ctx = fakeContext(undefined, async () => ({
    data: [
      {
        info: {
          id: "msg-1",
          role: "assistant",
          time: { completed: 1789897200000 },
        },
        parts: [{ type: "text", text: "真实完成正文" }],
      },
    ],
  }))
  const event = {
    type: "session.idle",
    properties: { sessionID: "session-idle" },
  }
  const submit = async (envelope) => submitted.push(envelope)

  assert.equal(__test.terminalEventType(event), "completed")
  assert.equal(__test.eventSessionID(event), "session-idle")
  await __test.dispatchTerminalEvent(ctx, "session-idle", event, submit)
  await __test.dispatchTerminalEvent(ctx, "session-idle", event, submit)

  assert.equal(submitted.length, 1)
  assert.equal(submitted[0].payload.body, "真实完成正文")
  assert.equal(
    submitted[0].payload.idempotencyKey,
    "opencode:session-idle:message:msg-1",
  )
})

test("OpenCode V2 event payload and assistant context are supported", async () => {
  const { __test } = await pluginModule
  __test.resetTerminalStateForTests()
  const submitted = []
  const ctx = fakeContext(undefined, async () => ({
    data: [
      {
        id: "msg-v2-assistant",
        type: "assistant",
        time: { completed: 1789897202000 },
        content: [{ type: "text", text: "V2 assistant 正文" }],
      },
    ],
  }))
  const event = {
    id: "evt-v2-idle",
    type: "session.idle",
    data: { sessionID: "session-v2" },
  }
  const submit = async (envelope) => submitted.push(envelope)

  assert.equal(__test.terminalEventType(event), "completed")
  assert.equal(__test.eventSessionID(event), "session-v2")
  await __test.dispatchTerminalEvent(ctx, "session-v2", event, submit)

  assert.equal(submitted.length, 1)
  assert.equal(submitted[0].payload.sessionId, "session-v2")
  assert.equal(submitted[0].payload.body, "V2 assistant 正文")
  assert.equal(
    submitted[0].payload.idempotencyKey,
    "opencode:session-v2:message:msg-v2-assistant",
  )
})

test("session.execution.failed reports V2 provider errors", async () => {
  const { __test } = await pluginModule
  __test.resetTerminalStateForTests()
  const submitted = []
  const ctx = fakeContext(undefined, async () => ({ data: [] }))
  const event = {
    id: "evt-v2-failed",
    type: "session.execution.failed",
    data: {
      sessionID: "session-v2-failed",
      error: {
        type: "provider.no-route",
        message: "Model unavailable: invalid/model",
      },
    },
  }
  const submit = async (envelope) => submitted.push(envelope)

  assert.equal(__test.terminalEventType(event), "failed")
  assert.equal(__test.eventSessionID(event), "session-v2-failed")
  await __test.dispatchTerminalEvent(
    ctx,
    "session-v2-failed",
    event,
    submit,
  )

  assert.equal(submitted.length, 1)
  assert.match(submitted[0].payload.title, /任务失败/)
  assert.equal(
    submitted[0].payload.body,
    "任务执行失败：Model unavailable: invalid/model",
  )
  assert.equal(
    submitted[0].payload.idempotencyKey,
    "opencode:session-v2-failed:evt-v2-failed",
  )
})

test("session.error reports failure and suppresses the same idle round", async () => {
  const { __test } = await pluginModule
  __test.resetTerminalStateForTests()
  const submitted = []
  const ctx = fakeContext(undefined, async () => ({
    data: [
      {
        info: {
          id: "msg-error",
          role: "assistant",
          time: { completed: 1789897201000 },
        },
        parts: [{ type: "text", text: "失败前的部分输出" }],
      },
    ],
  }))
  const submit = async (envelope) => submitted.push(envelope)
  const errorEvent = {
    type: "session.error",
    properties: {
      sessionID: "session-error",
      error: { name: "APIError", data: { message: "provider unavailable" } },
    },
  }

  await __test.dispatchTerminalEvent(
    ctx,
    "session-error",
    errorEvent,
    submit,
    1000,
  )
  await __test.dispatchTerminalEvent(
    ctx,
    "session-error",
    { type: "session.idle", properties: { sessionID: "session-error" } },
    submit,
    1500,
  )

  assert.equal(submitted.length, 1)
  assert.match(submitted[0].payload.title, /任务失败/)
  assert.match(submitted[0].payload.body, /provider unavailable/)
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

test("open=true job creates a real session and maps the orchestration sessionID", async () => {
  const { __test } = await pluginModule
  __test.resetSessionMapForTests()
  const created = []
  const promptCalls = []
  const ctx = fakeContext(async (input) => {
    promptCalls.push(input)
  })
  ctx.session.create = async (input) => {
    created.push(input)
    return { id: "ses_test_open_1" }
  }
  writeJob("job-open", "【task_9】Step 1 开工信封", {
    open: true,
    sessionID: "task-9-step-1",
  })
  await __test.processReplyJobs(ctx, "test-instance")

  assert.deepEqual(readResult("job-open"), { ok: true, error: "" })
  assert.equal(created.length, 1)
  assert.match(created[0].title, /task-9-step-1/)
  assert.deepEqual(promptCalls, [
    {
      sessionID: "ses_test_open_1",
      text: "【task_9】Step 1 开工信封",
      delivery: "steer",
    },
  ])

  // 映射必须落盘：续聊（open=false）与完成事件回传都依赖它；写入为新格式（带 unattended）。
  const mapFile = path.join(replyDir, "session-map.json")
  const map = JSON.parse(fs.readFileSync(mapFile, "utf8"))
  assert.deepEqual(map["task-9-step-1"], {
    id: "ses_test_open_1",
    unattended: true,
  })
})

test("open=false reuses the mapped real session for orchestration ids", async () => {
  const { __test } = await pluginModule
  __test.resetSessionMapForTests()
  const promptCalls = []
  const ctx = fakeContext(async (input) => {
    promptCalls.push(input)
  })
  ctx.session.create = async () => ({ id: "ses_resume_mapped" })
  const real = await __test.resolvePromptSessionID(ctx, {
    id: "seed",
    sessionID: "task-map-step-1",
    text: "seed",
    createdAt: "",
    expiresAt: "",
    open: true,
  })
  assert.equal(real, "ses_resume_mapped")

  writeJob("job-resume-mapped", "继续处理", {
    open: false,
    sessionID: "task-map-step-1",
  })
  await __test.processReplyJobs(ctx, "test-instance")

  assert.deepEqual(readResult("job-resume-mapped"), { ok: true, error: "" })
  assert.deepEqual(promptCalls, [
    { sessionID: "ses_resume_mapped", text: "继续处理", delivery: "steer" },
  ])
})

test("open=false without a mapping creates a session for the step", async () => {
  const { __test } = await pluginModule
  __test.resetSessionMapForTests()
  const created = []
  const promptCalls = []
  const ctx = fakeContext(async (input) => {
    promptCalls.push(input)
  })
  ctx.session.create = async (input) => {
    created.push(input)
    return { id: "ses_step2_created" }
  }
  writeJob("job-resume-unmapped", "Step 2 规划信封", {
    open: false,
    sessionID: "task-unmapped-step-2",
  })
  await __test.processReplyJobs(ctx, "test-instance")

  assert.deepEqual(readResult("job-resume-unmapped"), { ok: true, error: "" })
  assert.equal(created.length, 1)
  assert.match(created[0].title, /task-unmapped-step-2/)
  assert.deepEqual(promptCalls, [
    {
      sessionID: "ses_step2_created",
      text: "Step 2 规划信封",
      delivery: "steer",
    },
  ])
})

test("open=true passes location and model to session.create and stores unattended", async () => {
  const { __test } = await pluginModule
  __test.resetSessionMapForTests()
  const created = []
  const ctx = fakeContext(async () => {})
  ctx.session.create = async (input) => {
    created.push(input)
    return { id: "ses_dispatch_options" }
  }
  writeJob("job-dispatch-options", "【task_opt】Step 1 开工信封", {
    open: true,
    sessionID: "task-dispatch-opt-step-1",
    model: "anthropic/claude-sonnet-4-5",
    location: "D:\\工作区\\项目",
    unattended: false,
  })
  await __test.processReplyJobs(ctx, "test-instance")

  assert.deepEqual(readResult("job-dispatch-options"), { ok: true, error: "" })
  assert.deepEqual(created, [
    {
      title: "【集群】task-dispatch-opt-step-1",
      location: { directory: "D:\\工作区\\项目" },
      model: { providerID: "anthropic", id: "claude-sonnet-4-5" },
    },
  ])
  const map = JSON.parse(
    fs.readFileSync(path.join(replyDir, "session-map.json"), "utf8"),
  )
  assert.deepEqual(map["task-dispatch-opt-step-1"], {
    id: "ses_dispatch_options",
    unattended: false,
  })
})

test("invalid model spec fails the job before session.create", async () => {
  const { __test } = await pluginModule
  __test.resetSessionMapForTests()
  const created = []
  const ctx = fakeContext(async () => {})
  ctx.session.create = async (input) => {
    created.push(input)
    return { id: "ses_invalid_model" }
  }
  writeJob("job-invalid-model", "开工信封", {
    open: true,
    sessionID: "task-bad-model-step-1",
    model: "justmodel",
  })
  await __test.processReplyJobs(ctx, "test-instance")

  const result = readResult("job-invalid-model")
  assert.equal(result.ok, false)
  assert.match(result.error, /provider\/model/)
  assert.deepEqual(created, [])
})

test("open=false switches the model only when the job requests one", async () => {
  const { __test } = await pluginModule
  __test.resetSessionMapForTests()
  const promptCalls = []
  const switchCalls = []
  const ctx = fakeContext(async (input) => {
    promptCalls.push(input)
  })
  ctx.session.create = async () => ({ id: "ses_resume_model" })
  ctx.session.switchModel = async (input) => {
    switchCalls.push(input)
  }
  await __test.resolvePromptSessionID(ctx, {
    id: "seed-switch",
    sessionID: "task-switch-model-step-1",
    text: "seed",
    createdAt: "",
    expiresAt: "",
    open: true,
  })
  writeJob("job-switch-model", "继续处理", {
    open: false,
    sessionID: "task-switch-model-step-1",
    model: "anthropic/claude-sonnet-4-5",
  })
  writeJob("job-switch-default", "继续处理", {
    open: false,
    sessionID: "task-switch-model-step-1",
  })
  await __test.processReplyJobs(ctx, "test-instance")

  assert.deepEqual(readResult("job-switch-model"), { ok: true, error: "" })
  assert.deepEqual(readResult("job-switch-default"), { ok: true, error: "" })
  assert.deepEqual(switchCalls, [
    {
      sessionID: "ses_resume_model",
      model: { providerID: "anthropic", id: "claude-sonnet-4-5" },
    },
  ])
  assert.equal(promptCalls.length, 2)
})

test("resume with model fails clearly when the host cannot switch models", async () => {
  const { __test } = await pluginModule
  __test.resetSessionMapForTests()
  const ctx = fakeContext(async () => {})
  ctx.session.create = async () => ({ id: "ses_no_switch" })
  await __test.resolvePromptSessionID(ctx, {
    id: "seed-no-switch",
    sessionID: "task-no-switch-step-1",
    text: "seed",
    createdAt: "",
    expiresAt: "",
    open: true,
  })
  writeJob("job-no-switch", "继续处理", {
    open: false,
    sessionID: "task-no-switch-step-1",
    model: "anthropic/claude-sonnet-4-5",
  })
  await __test.processReplyJobs(ctx, "test-instance")

  const result = readResult("job-no-switch")
  assert.equal(result.ok, false)
  assert.match(result.error, /不支持切换模型/)
})

test("open=false updates the unattended flag in the session map", async () => {
  const { __test } = await pluginModule
  __test.resetSessionMapForTests()
  const ctx = fakeContext(async () => {})
  ctx.session.create = async () => ({ id: "ses_flag_update" })
  await __test.resolvePromptSessionID(ctx, {
    id: "seed-flag",
    sessionID: "task-flag-step-1",
    text: "seed",
    createdAt: "",
    expiresAt: "",
    open: true,
  })
  const mapFile = path.join(replyDir, "session-map.json")
  assert.deepEqual(
    JSON.parse(fs.readFileSync(mapFile, "utf8"))["task-flag-step-1"],
    { id: "ses_flag_update", unattended: true },
  )

  writeJob("job-flag-update", "继续处理", {
    open: false,
    sessionID: "task-flag-step-1",
    unattended: false,
  })
  await __test.processReplyJobs(ctx, "test-instance")

  assert.deepEqual(readResult("job-flag-update"), { ok: true, error: "" })
  assert.deepEqual(
    JSON.parse(fs.readFileSync(mapFile, "utf8"))["task-flag-step-1"],
    { id: "ses_flag_update", unattended: false },
  )
})

test("completion event carries the orchestration sessionID after mapping", async () => {
  const { __test } = await pluginModule
  __test.resetTerminalStateForTests()
  __test.resetSessionMapForTests()
  const ctx = fakeContext(undefined, async () => ({
    data: [
      {
        info: {
          id: "msg-orc-1",
          role: "assistant",
          time: { completed: 1789897203000 },
        },
        parts: [{ type: "text", text: "第 1 步完成" }],
      },
    ],
  }))
  ctx.session.create = async () => ({ id: "ses_event_mapped" })
  await __test.resolvePromptSessionID(ctx, {
    id: "seed",
    sessionID: "task-ev-step-3",
    text: "seed",
    createdAt: "",
    expiresAt: "",
    open: true,
  })

  const submitted = []
  const event = {
    type: "session.idle",
    properties: { sessionID: "ses_event_mapped" },
  }
  await __test.dispatchTerminalEvent(
    ctx,
    "ses_event_mapped",
    event,
    async (envelope) => submitted.push(envelope),
  )

  assert.equal(submitted.length, 1)
  assert.equal(submitted[0].payload.sessionId, "task-ev-step-3")
  assert.match(submitted[0].payload.idempotencyKey, /^opencode:task-ev-step-3:/)
})

test("open=false or missing open resumes the existing session unchanged", async () => {
  const { __test } = await pluginModule
  const promptCalls = []
  const ctx = fakeContext(async (input) => {
    promptCalls.push(input)
  })
  writeJob("job-resume-false", "继续处理", { open: false })
  writeJob("job-resume-missing", "继续处理")
  await __test.processReplyJobs(ctx, "test-instance")

  assert.deepEqual(readResult("job-resume-false"), { ok: true, error: "" })
  assert.deepEqual(readResult("job-resume-missing"), { ok: true, error: "" })
  assert.deepEqual(promptCalls, [
    { sessionID: "session-1", text: "继续处理", delivery: "steer" },
    { sessionID: "session-1", text: "继续处理", delivery: "steer" },
  ])
})

test("open=true without session.create fails with a clear message", async () => {
  const { __test } = await pluginModule
  __test.resetSessionMapForTests()
  const legacyCtx = {
    session: {
      get: async () => ({ data: { title: "插件测试" } }),
      context: async () => ({ data: [] }),
    },
    client: { session: { promptAsync: async () => {} } },
    event: {
      subscribe: async function* subscribe() {
        yield* []
      },
    },
  }
  writeJob("job-open-legacy", "开工信封", {
    open: true,
    sessionID: "task-legacy-step-1",
  })
  await __test.processReplyJobs(legacyCtx, "test-instance")

  const result = readResult("job-open-legacy")
  assert.equal(result.ok, false)
  assert.match(result.error, /不支持创建会话/)
})

test("ingress failure is swallowed and does not reject OpenCode", async () => {
  const { __test } = await pluginModule
  const ctx = fakeContext(async () => {})
  await assert.doesNotReject(() =>
    __test.dispatchCompletion(ctx, "session-1", { id: "event-10" }),
  )
})

test("stale instance retires when a newer heartbeat exists", async () => {
  const { __test } = await pluginModule
  const hbDir = path.join(replyDir, "heartbeats")
  fs.mkdirSync(hbDir, { recursive: true })
  for (const name of fs.readdirSync(hbDir)) {
    fs.rmSync(path.join(hbDir, name), { force: true })
  }
  const now = Date.now()
  const older = `9001-${(now - 60_000).toString(36)}-aaaa`
  const newer = `9001-${now.toString(36)}-bbbb`
  fs.writeFileSync(path.join(hbDir, `${older}.json`), "{}")
  fs.writeFileSync(path.join(hbDir, `${newer}.json`), "{}")

  assert.ok(
    __test.instanceCreatedAt(newer) > __test.instanceCreatedAt(older),
    "实例创建时间应可比较",
  )
  assert.equal(__test.supersededByNewerInstance(older), true, "旧实例应退休");
  assert.equal(__test.supersededByNewerInstance(newer), false, "新实例不应退休")

  // 更新的心跳过期后不再触发退休（避免误退与陈旧文件互锁）。
  const past = new Date(now - 60_000)
  fs.utimesSync(path.join(hbDir, `${newer}.json`), past, past)
  assert.equal(__test.supersededByNewerInstance(older), false, "过期心跳不触发退休")

  for (const name of fs.readdirSync(hbDir)) {
    fs.rmSync(path.join(hbDir, name), { force: true })
  }
})

test("session map refresh picks up mappings written after first load", async () => {
  const { __test } = await pluginModule
  __test.resetSessionMapForTests()
  __test.setSessionMapRefreshIntervalForTests(0)
  const mapFile = path.join(replyDir, "session-map.json")
  fs.writeFileSync(mapFile, JSON.stringify({}))
  // 首次查询建立缓存（空映射）。
  assert.equal(__test.mappedSessionID("ses_missing"), "ses_missing")
  // 模拟其他（热重载前）实例写入映射：本实例未命中时应刷新磁盘一次再找。
  fs.writeFileSync(
    mapFile,
    JSON.stringify({ "task-refresh-step-1": "ses_refresh_1" }),
  )
  assert.equal(
    __test.mappedSessionID("ses_refresh_1"),
    "task-refresh-step-1",
    "未命中时应刷新磁盘映射",
  )
})

test("permission ask is auto-allowed for orchestration sessions", async () => {
  const { __test } = await pluginModule
  __test.resetSessionMapForTests()
  const ctx = fakeContext(async () => {})
  ctx.session.create = async () => ({ id: "ses_perm_mapped" })
  await __test.resolvePromptSessionID(ctx, {
    id: "seed-perm",
    sessionID: "task-perm-step-1",
    text: "seed",
    createdAt: "",
    expiresAt: "",
    open: true,
  })

  const event = {
    sessionID: "ses_perm_mapped",
    action: "edit",
    resources: ["src/app.ts"],
    effect: "ask",
  }
  __test.evaluatePermission(event)
  assert.equal(event.effect, "allow")
})

test("permission ask is kept for unattended=false orchestration sessions", async () => {
  const { __test } = await pluginModule
  __test.resetSessionMapForTests()
  const ctx = fakeContext(async () => {})
  ctx.session.create = async () => ({ id: "ses_perm_manual" })
  await __test.resolvePromptSessionID(ctx, {
    id: "seed-manual",
    sessionID: "task-manual-step-1",
    text: "seed",
    createdAt: "",
    expiresAt: "",
    open: true,
    unattended: false,
  })

  const manual = {
    sessionID: "ses_perm_manual",
    action: "bash",
    resources: ["rm -rf build"],
    effect: "ask",
  }
  __test.evaluatePermission(manual)
  assert.equal(manual.effect, "ask", "unattended=false 必须保留人工确认")

  // unattended 缺省的编排会话仍自动放行。
  ctx.session.create = async () => ({ id: "ses_perm_default" })
  await __test.resolvePromptSessionID(ctx, {
    id: "seed-default",
    sessionID: "task-default-step-1",
    text: "seed",
    createdAt: "",
    expiresAt: "",
    open: true,
  })
  const automatic = {
    sessionID: "ses_perm_default",
    action: "bash",
    resources: [],
    effect: "ask",
  }
  __test.evaluatePermission(automatic)
  assert.equal(automatic.effect, "allow")
})

test("legacy string session map entries stay readable and unattended", async () => {
  const { __test } = await pluginModule
  __test.resetSessionMapForTests()
  fs.writeFileSync(
    path.join(replyDir, "session-map.json"),
    JSON.stringify({ "task-legacy-step-1": "ses_legacy_mapped" }),
  )
  __test.setSessionMapRefreshIntervalForTests(0)

  assert.equal(
    __test.mappedSessionID("ses_legacy_mapped"),
    "task-legacy-step-1",
  )
  const event = {
    sessionID: "ses_legacy_mapped",
    action: "edit",
    resources: [],
    effect: "ask",
  }
  __test.evaluatePermission(event)
  assert.equal(event.effect, "allow")
})

test("permission ask is untouched for plain sessions", async () => {
  const { __test } = await pluginModule
  __test.resetSessionMapForTests()
  const event = {
    sessionID: "ses_plain_unmapped",
    action: "bash",
    resources: ["rm -rf build"],
    effect: "ask",
  }
  __test.evaluatePermission(event)
  assert.equal(event.effect, "ask")
})

test("permission allow and deny effects are never rewritten", async () => {
  const { __test } = await pluginModule
  __test.resetSessionMapForTests()
  const ctx = fakeContext(async () => {})
  ctx.session.create = async () => ({ id: "ses_perm_effects" })
  await __test.resolvePromptSessionID(ctx, {
    id: "seed-effects",
    sessionID: "task-effects-step-1",
    text: "seed",
    createdAt: "",
    expiresAt: "",
    open: true,
  })

  const allowed = {
    sessionID: "ses_perm_effects",
    action: "read",
    resources: [],
    effect: "allow",
  }
  const denied = {
    sessionID: "ses_perm_effects",
    action: "bash",
    resources: [],
    effect: "deny",
  }
  __test.evaluatePermission(allowed)
  __test.evaluatePermission(denied)
  assert.equal(allowed.effect, "allow")
  assert.equal(denied.effect, "deny")
})

test("setup tolerates a missing or failing permission API", async () => {
  const { default: plugin } = await pluginModule

  // 宿主无 permission API：setup 正常完成。
  const noApiCtx = fakeContext(async () => {})
  assert.equal(noApiCtx.permission, undefined)
  const disposeNoApi = await plugin.setup(noApiCtx)
  await disposeNoApi()

  // 注册即抛错：降级且不阻断 setup。
  const brokenCtx = fakeContext(async () => {})
  brokenCtx.permission = {
    hook: () => {
      throw new Error("permission hook boom")
    },
  }
  const disposeBroken = await plugin.setup(brokenCtx)
  await disposeBroken()
})

test("setup registers the permission evaluate hook and disposes it", async () => {
  const { default: plugin } = await pluginModule
  const registered = []
  let disposed = 0
  const ctx = fakeContext(async () => {})
  ctx.permission = {
    hook: async (name, handler) => {
      registered.push({ name, handler })
      return {
        dispose: () => {
          disposed += 1
        },
      }
    },
  }

  const dispose = await plugin.setup(ctx)
  assert.equal(registered.length, 1)
  assert.equal(registered[0].name, "evaluate")

  await dispose()
  await waitFor(() => disposed === 1)
})
