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
 * 环境变量（改完要重启 opencode 桌面端才生效）：
 * - AGENT_NOTIFY_BIN：agent-notify.exe 路径，默认 %USERPROFILE%\bin\agent-notify.exe
 * - AGENT_NOTIFY_OPENCODE_MARKER_FILE：开关 marker，存在即停，默认
 *   %USERPROFILE%\.config\agent-notify\opencode.off
 * - AGENT_NOTIFY_COOLDOWN_MIN：同会话冷却分钟数，默认读配置文件，兜底 10
 * - AGENT_NOTIFY_DRYRUN=1：只渲染不发送（联调用）
 * - AGENT_NOTIFY_OFF=1：插件总开关，直接不拉起 CLI
 * - AGENT_NOTIFY_DEBUG=1：把判定与调用结果写进 %TEMP%\agent-notify\opencode-debug.log
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

const RAW_CHARS = 2000
const DEFAULT_COOLDOWN_MIN = 10

let fsMod = null
let debugLog = null
let cooldownMs = null

function envValue(name) {
  return (typeof process !== "undefined" && process.env[name]) || ""
}

function resolveTarget() {
  const configured = envValue("AGENT_NOTIFY_BIN")
  if (configured) {
    return configured
  }
  const home = envValue("USERPROFILE")
  if (home) {
    const installed = `${home}\\bin\\agent-notify.exe`
    try {
      if (fsMod && fsMod.existsSync(installed)) {
        return installed
      }
    } catch {
      /* 探测失败按 PATH 兜底 */
    }
  }
  return "agent-notify.exe"
}

async function fsAsync() {
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

function dbg(msg) {
  try {
    if (debugLog) {
      debugLog(msg)
    }
  } catch {
    /* 日志失败不影响主流程 */
  }
}

// 冷却时间来源优先级：环境变量 > config.json > 默认值。
function cooldown() {
  if (cooldownMs !== null) {
    return cooldownMs
  }
  let minutes = Number(envValue("AGENT_NOTIFY_COOLDOWN_MIN")) || 0
  if (!minutes && CONFIG_FILE && fsMod) {
    try {
      const raw = fsMod.readFileSync(CONFIG_FILE, "utf8")
      const parsed = JSON.parse(raw)
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
  cooldownMs = minutes * 60_000
  return cooldownMs
}

function markerOff() {
  try {
    if (!MARKER_FILE || !fsMod) {
      return false
    }
    return fsMod.existsSync(MARKER_FILE)
  } catch {
    return false
  }
}

function readSent() {
  try {
    if (!fsMod) {
      return {}
    }
    const parsed = JSON.parse(fsMod.readFileSync(STATE_FILE, "utf8"))
    return isRecord(parsed) ? parsed : {}
  } catch {
    return {}
  }
}

function writeSent(map) {
  try {
    if (fsMod) {
      fsMod.writeFileSync(STATE_FILE, JSON.stringify(map))
    }
  } catch {
    /* 状态落盘失败不影响推送 */
  }
}

function isRecord(value) {
  return typeof value === "object" && value !== null
}

function spawnNotify(title, summary, sessionID) {
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
    "800",
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
    const timer = setTimeout(done, 30000)
    import("node:child_process")
      .then(({ execFile }) => {
        const child = execFile(
          target,
          args,
          { timeout: 25000, windowsHide: true },
          (error, stdout, stderr) => {
            clearTimeout(timer)
            dbg(
              `exit sid=${sessionID} err=${(error && String(error.message || error)) || "none"} stderr=${String(stderr || "").slice(0, 200)} out=${String(stdout || "").slice(0, 200)}`,
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
          dbg(
            `error sid=${sessionID} ${String((error && error.message) || error)}`,
          )
          done()
        })
      })
      .catch(done)
  })
}

/** 取会话最后一条 assistant 文本，截断到上限；拿不到返回空串。 */
async function lastAssistantText(session, sessionID) {
  const res = await session.context({ sessionID })
  const data = Array.isArray(res) ? res : res && res.data
  if (!Array.isArray(data)) {
    return ""
  }
  for (let i = data.length - 1; i >= 0; i--) {
    const item = data[i]
    if (!isRecord(item)) {
      continue
    }
    const role = isRecord(item.info)
      ? item.info.role
      : typeof item.role === "string"
        ? item.role
        : item.type
    if (role !== "assistant") {
      continue
    }
    if (typeof item.text === "string") {
      const text = item.text.trim()
      if (text.length > 0) {
        return text.slice(0, RAW_CHARS)
      }
      continue
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
            part.text.trim().length > 0,
        )
        .map((part) => part.text)
        .join("\n")
        .trim()
      if (text.length > 0) {
        return text.slice(0, RAW_CHARS)
      }
    }
  }
  return ""
}

async function sessionTitle(session, sessionID) {
  const res = await session.get({ sessionID })
  const info = Array.isArray(res) ? undefined : res && res.data ? res.data : res
  const title = isRecord(info) ? info.title : undefined
  return typeof title === "string" && title.trim().length > 0
    ? title.trim().slice(0, 80)
    : "opencode会话"
}

const lastSent = new Map()
const pending = new Set()

async function handleTaskComplete(ctx, sessionID) {
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
  if (lastSent.size > 500) {
    lastSent.clear()
  }
  sent[sessionID] = now
  for (const key of Object.keys(sent)) {
    if (now - Number(sent[key]) > wait) {
      delete sent[key]
    }
  }
  writeSent(sent)

  let title = "opencode会话"
  try {
    title = await sessionTitle(ctx.session, sessionID)
  } catch (error) {
    dbg(
      `title fail sid=${sessionID} err=${String((error && error.message) || error)}`,
    )
  }

  let summary = ""
  try {
    summary = await lastAssistantText(ctx.session, sessionID)
  } catch (error) {
    dbg(
      `summary fail sid=${sessionID} err=${String((error && error.message) || error)}`,
    )
  }

  try {
    await spawnNotify(`【opencode】${title}`, summary, sessionID)
    dbg(`pushed sid=${sessionID}`)
  } finally {
    pending.delete(sessionID)
  }
}

export default {
  id: "agent-notify",
  setup: async (ctx) => {
    await fsAsync()
    try {
      if (envValue("AGENT_NOTIFY_DEBUG") === "1") {
        const fs = await import("node:fs")
        debugLog = (msg) => {
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
      controller.abort()
    }
  },
}
