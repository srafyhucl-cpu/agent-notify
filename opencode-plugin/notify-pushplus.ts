/**
 * 任务完成微信推送（PushPlus）：会话进入空闲时推一条带摘要的消息。
 *
 * 形状：opencode 桌面端加载器要求的 V2 定义 `{ id, setup }`，零 import，
 * 不依赖 node_modules 里版本不一致的 `@opencode-ai/plugin`。
 *
 * 触发：`session.execution.succeeded`（桌面端实测的任务完成事件；
 * `session.idle` 在此版本不出现，保留做兼容）。
 * 摘要：取该会话最近消息里最后一条 assistant 文本，压缩成一行。
 * 去重：同一会话按冷却时间只推一次，活跃聊天时不会刷屏。
 * 安全：失败全部吞掉，永远不影响 agent 运行；token 只从环境变量读。
 *
 * 环境变量（桌面端 service 进程的环境，改完要重启桌面端）：
 * - PUSHPLUS_TOKEN（必填）：PushPlus token，无则静默跳过且不计冷却。
 * - OPENCODE_NOTIFY_SCRIPT / NOTIFY_AI_SCRIPT：推送脚本路径，默认 %USERPROFILE%\bin\notify-ai.ps1。
 * - OPENCODE_NOTIFY_COOLDOWN_MIN：同会话冷却分钟数，默认 10。
 * - OPENCODE_NOTIFY_DRYRUN=1：只调脚本加 -DryRun，不真推（联调用）。
 * - OPENCODE_NOTIFY_OFF=1：总开关，关闭推送。
 */

function defaultBinScript() {
  const up =
    (typeof process !== "undefined" && process.env.USERPROFILE) || ""
  return up ? `${up}\\bin\\notify-ai.ps1` : "notify-ai.ps1"
}
const SCRIPT =
  (typeof process !== "undefined" &&
    (process.env.OPENCODE_NOTIFY_SCRIPT || process.env.NOTIFY_AI_SCRIPT)) ||
  defaultBinScript()
const COOLDOWN_MS =
  (Number(
    (typeof process !== "undefined" &&
      process.env.OPENCODE_NOTIFY_COOLDOWN_MIN) ||
      "10",
  ) || 10) * 60_000
const RAW_CHARS = 2000

const lastSent = new Map()
const pending = new Set()

// 跨实例冷却：内存 Map 只对同一模块实例有效，桌面端多 location
// setup 各自订阅事件时会收到重复投递，用共享状态文件去重。
// 测试可用 OPENCODE_NOTIFY_STATE_FILE / OPENCODE_NOTIFY_LOG_FILE 隔离。
const TMP_BASE =
  (typeof process !== "undefined" &&
    (process.env.TEMP ||
      process.env.TMP ||
      (process.env.USERPROFILE
        ? `${process.env.USERPROFILE}/AppData/Local/Temp`
        : "C:/Temp"))) ||
  "C:/Temp"
const STATE_FILE =
  (typeof process !== "undefined" && process.env.OPENCODE_NOTIFY_STATE_FILE) ||
  `${TMP_BASE}/opencode/notify-push-sent.json`
const PUSH_LOG_FILE =
  (typeof process !== "undefined" && process.env.OPENCODE_NOTIFY_LOG_FILE) ||
  `${TMP_BASE}/opencode/notify-push.log`
let fsMod = null
async function fsAsync() {
  if (!fsMod) {
    try {
      fsMod = await import("node:fs")
      try {
        fsMod.mkdirSync(`${TMP_BASE}/opencode`, { recursive: true })
      } catch {
        /* 目录建失败不影响主流程 */
      }
    } catch {
      fsMod = null
    }
  }
  return fsMod
}
function readSent() {
  try {
    if (!fsMod) return {}
    const raw = fsMod.readFileSync(STATE_FILE, "utf8")
    const obj = JSON.parse(raw)
    return isRecord(obj) ? obj : {}
  } catch {
    return {}
  }
}
function writeSent(map) {
  try {
    if (fsMod) fsMod.writeFileSync(STATE_FILE, JSON.stringify(map))
  } catch {
    /* 状态落盘失败不影响推送 */
  }
}
function logPush(sessionID, title) {
  try {
    if (fsMod)
      fsMod.appendFileSync(
        PUSH_LOG_FILE,
        `${new Date().toISOString()} push sid=${sessionID} title=${title}\n`,
      )
  } catch {
    /* 日志失败不影响推送 */
  }
}

let debugLog = null
function dbg(msg) {
  try {
    if (debugLog) debugLog(msg)
  } catch {
    /* 日志失败不影响主流程 */
  }
}

function isRecord(v) {
  return typeof v === "object" && v !== null
}

function spawnNotify(title, summary) {
  const dry = process.env.OPENCODE_NOTIFY_DRYRUN === "1"
  const args = [
    "-NoProfile",
    "-ExecutionPolicy",
    "Bypass",
    "-File",
    SCRIPT,
    "-Title",
    `【opencode】${title}`,
    "-Summary",
    summary,
    "-NoStdin",
  ]
  if (dry) args.push("-DryRun")
  return new Promise((resolve) => {
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
          "powershell.exe",
          args,
          {
            timeout: 25000,
            windowsHide: true,
            // 必须关闭 stdin：notify-ai.ps1 无 -Summary 时会读 stdin，
            // 管道不关它就一直等到超时。
            input: "",
          },
          (error, stdout, stderr) => {
            try {
              dbg(
                `cb sid=${title} err=${(error && String(error.message || error)) || "none"} stderr=${String(stderr || "").slice(0, 200)} out=${String(stdout || "").slice(0, 200)}`,
              )
            } catch {
              /* 忽略日志失败 */
            }
            clearTimeout(timer)
            done()
          },
        )
        child.on("error", () => {
          clearTimeout(timer)
          done()
        })
      })
      .catch(done)
  })
}

/** 取会话最后一条 assistant 文本，压成一行并截断；拿不到返回空串。 */
async function lastAssistantText(session, sessionID) {
  const res = await session.context({ sessionID })
  const data = Array.isArray(res) ? res : res && res.data
  if (!Array.isArray(data)) return ""
  try {
    const probe = data
      .slice(-3)
      .map((m) => {
        if (!isRecord(m)) return typeof m
        const parts = m.parts
        const pinfo = Array.isArray(parts)
          ? `plen=${parts.length} ptypes=[${parts.map((p) => (isRecord(p) ? String(p.type) : typeof p)).join(",")}] pkeys=${isRecord(parts[0]) ? Object.keys(parts[0]).join(",") : typeof parts[0]}`
          : `parts=${typeof parts}`
        return `type=${String(m.type)} role=${isRecord(m.info) ? String(m.info.role) : String(m.role)} keys=${Object.keys(m).join(",")} textlen=${typeof m.text === "string" ? m.text.length : -1} ${pinfo}`
      })
      .join(" | ")
    dbg(`ctx sample sid=${sessionID} ${probe}`)
    const lastA = [...data]
      .reverse()
      .find(
        (m) =>
          isRecord(m) &&
          (String(m.type) === "assistant" ||
            (isRecord(m.info) && String(m.info.role) === "assistant")),
      )
    if (isRecord(lastA)) {
      try {
        dbg(
          `ctx dump sid=${sessionID} ${JSON.stringify(lastA).slice(0, 600)}`,
        )
      } catch {
        /* 忽略日志失败 */
      }
    }
  } catch {
    /* 忽略日志失败 */
  }
  for (let i = data.length - 1; i >= 0; i--) {
    const item = data[i]
    if (!isRecord(item)) continue
    const role = isRecord(item.info)
      ? item.info.role
      : typeof item.role === "string"
        ? item.role
        : item.type
    if (role !== "assistant") continue
    // 文本来源 1：扁平 text 字段（多为 user 消息，assistant 偶尔也有）。
    // 只去首尾空、保留原文排版，结构化渲染由 notify-ai.ps1 统一做。
    if (typeof item.text === "string") {
      const text = item.text.trim()
      if (text.length > 0) return text.slice(0, RAW_CHARS)
      continue
    }
    // 文本来源 2：parts/content 数组里的 text 项（assistant 主要形态，
    // 注意跳过 reasoning/tool 项）。
    const buckets = [item.parts, item.content]
    for (const bucket of buckets) {
      if (!Array.isArray(bucket)) continue
      const text = bucket
        .filter(
          (p) =>
            isRecord(p) &&
            p.type === "text" &&
            typeof p.text === "string" &&
            p.text.trim().length > 0,
        )
        .map((p) => p.text)
        .join("\n")
        .trim()
      if (text.length > 0) return text.slice(0, RAW_CHARS)
    }
    continue
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

async function handleIdle(ctx, sessionID) {
  if (process.env.OPENCODE_NOTIFY_OFF === "1") {
    dbg("skip: OFF=1")
    return
  }
  if (typeof sessionID !== "string" || sessionID.length === 0) {
    dbg("skip: bad sessionID")
    return
  }
  if (!process.env.PUSHPLUS_TOKEN) {
    dbg(`skip: no PUSHPLUS_TOKEN sid=${sessionID}`)
    return
  }
  const now = Date.now()
  if (now - (lastSent.get(sessionID) || 0) < COOLDOWN_MS) {
    dbg(`skip: cooldown sid=${sessionID}`)
    return
  }
  if (pending.has(sessionID)) {
    dbg(`skip: in-flight sid=${sessionID}`)
    return
  }
  const sent = readSent()
  if (now - (Number(sent[sessionID]) || 0) < COOLDOWN_MS) {
    lastSent.set(sessionID, now)
    dbg(`skip: file-cooldown sid=${sessionID}`)
    return
  }
  pending.add(sessionID)
  lastSent.set(sessionID, now)
  if (lastSent.size > 500) lastSent.clear()
  sent[sessionID] = now
  for (const k of Object.keys(sent)) {
    if (now - Number(sent[k]) > COOLDOWN_MS) delete sent[k]
  }
  writeSent(sent)

  let title = "opencode会话"
  let summary = ""
  try {
    title = await sessionTitle(ctx.session, sessionID)
    dbg(`title ok sid=${sessionID} len=${title.length}`)
  } catch (e) {
    dbg(`title fail sid=${sessionID} err=${String((e && e.message) || e)}`)
  }
  try {
    summary = await lastAssistantText(ctx.session, sessionID)
    dbg(`summary ok sid=${sessionID} len=${summary.length}`)
  } catch (e) {
    dbg(`summary fail sid=${sessionID} err=${String((e && e.message) || e)}`)
  }
  try {
    dbg(`spawn sid=${sessionID}`)
    await spawnNotify(title, summary)
    dbg(`spawned sid=${sessionID}`)
    logPush(sessionID, title)
  } finally {
    pending.delete(sessionID)
  }
}

export default {
  id: "notify-pushplus",
  setup: async (ctx) => {
    await fsAsync()
    try {
      if (process.env.OPENCODE_NOTIFY_DEBUG === "1") {
        const fs = await import("node:fs")
        const file = `${TMP_BASE}/opencode/notify-debug.log`
        debugLog = (msg) => {
          fs.appendFileSync(file, `${new Date().toISOString()} ${msg}\n`)
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
            if (!isRecord(event)) continue
            const t = event.type
            if (t !== "session.execution.succeeded" && t !== "session.idle")
              continue
            const props = event.properties || event.data
            const direct = event.sessionID
            const sessionID =
              (isRecord(props) && typeof props.sessionID === "string"
                ? props.sessionID
                : "") ||
              (typeof direct === "string" ? direct : "")
            if (!sessionID) {
              try {
                dbg(
                  `no-sid type=${String(t)} keys=${Object.keys(event).join(",")} props=${isRecord(props) ? Object.keys(props).join(",") : typeof props}`,
                )
              } catch {
                /* 忽略日志失败 */
              }
              continue
            }
            void handleIdle(ctx, sessionID)
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
