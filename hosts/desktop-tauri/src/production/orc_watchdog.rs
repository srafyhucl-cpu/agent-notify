//! 宿主侧编排看门狗（§4.4 兜底）：OpenCode 插件实例可能被回收/未加载，导致会话完成事件
//! （`session.idle` → `session.completed`）丢失，任务停在当前步不推进。
//!
//! 看门狗用 OpenCode 只读 HTTP API 定期核对「当前步的会话是否已经跑完一回合、且产出在本次
//! 派活之后」：满足条件而宿主还没收到汇报时，按与插件相同的事件语义回注
//! （复用 [`OrcCommandHandler::report_from_agent`]，推进/呈现/派活同一条链路）。
//!
//! 保守原则（宁可晚、不可错）：
//! - 只处理「已开始、干活中、未汇总、未阻塞」的任务；
//! - 只认本回合（最后一条 user 之后）且完成时间**晚于最近一次派活时刻**的 assistant 产出；
//! - 回合结束后留 [`WATCHDOG_GRACE_MS`] 宽限，先让插件正常上报，避免竞争；
//! - 旧任务没有派活时刻（升级前）先写入基线并跳过，不猜历史回合；
//! - 同一回合回注过（`mark_settled_turn`）不再重复。

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use agentnotify_domain::Timestamp;
use agentnotify_orchestration::{OrcTask, OrcTaskRepository, TaskState};
use serde_json::Value;

use super::orc_handler::OrcCommandHandler;

/// 巡检间隔：每 30 秒核对一次运行中的任务。
const WATCHDOG_INTERVAL: Duration = Duration::from_secs(30);
/// 回合结束后宽限：先让插件正常上报（正常几秒内到），超过宽限才兜底回注。
const WATCHDOG_GRACE_MS: i64 = 60_000;
/// 回合有响应但没有任何文字时的失败原因（与插件同语义：宿主阻塞 + 可读建议）。
const NO_REPORT_FAILURE: &str = "本回合已结束，但没有产出汇报（模型可能提前中断）：请点「重新发起」让它继续，或打开会话检查产出。";
/// 会话映射文件（插件维护：逻辑会话 id → 真实 OpenCode 会话 id）。
const SESSION_MAP_FILE: &str = "session-map.json";
/// 回复收件箱目录（相对用户目录；`AGENT_NOTIFY_OPENCODE_REPLY_DIR` 可覆盖）。
const REPLY_DIR_RELATIVE: &str = ".config/agent-notify/opencode-reply-inbox";
/// 只读请求超时。
const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);

/// 本回合扫描结果（与插件 `scanTurnAssistantMessages` 同语义）。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TurnScan {
    /// 本回合是否出现过 assistant 消息（区分「没响应」与「响应了但没正文」）
    pub saw_assistant: bool,
    /// 本回合是否已收束（会话尾部有 idle 标记；回合进行中不得回注）
    pub closed: bool,
    /// 本回合最新一条带正文的 assistant 消息正文
    pub text: Option<String>,
    /// 该回合（最新 assistant 消息）的完成时间（ms epoch）
    pub completed_at_ms: Option<i64>,
}

/// 看门狗判定结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WatchdogDecision {
    /// 无需动作（回合未结束 / 产出早于派活 / 还在宽限期 / 无可信产出）
    None,
    /// 回注该回合产出（正文 + 是否失败）
    Report {
        body: String,
        completed_at_ms: i64,
        failed: bool,
    },
}

/// 扫描会话消息尾部，取本回合（最后一条 user 之后）的 assistant 产出。
///
/// 消息项形状兼容两种 OpenCode 返回：`{info: {...}, parts/content: [...]}` 与
/// `{id, type, time, content: [...]}`；顺序不做假设（按 `time.created` 排序）。
///
/// `closed` 只认「最新一条消息是 idle 标记」：OpenCode 在会话收束（回合结束）时追加
/// idle 项；回合进行中（还在工具调用/思考）尾部仍是 assistant 消息，此时**不得回注**，
/// 否则会把进行中的旁白当成最终汇报（2026-09-29 真机事故）。
pub fn scan_turn(items: &[Value]) -> TurnScan {
    let mut ordered: Vec<&Value> = items.iter().collect();
    ordered.sort_by_key(|item| value_field_i64(item, &["time", "created"]).unwrap_or(0));

    let mut scan = TurnScan::default();
    if let Some(newest) = ordered.last() {
        scan.closed = message_kind(newest).as_deref() == Some("idle");
    }
    for item in ordered.iter().rev() {
        let kind = message_kind(item);
        if kind.as_deref() == Some("user") {
            break;
        }
        if kind.as_deref() != Some("assistant") {
            continue;
        }
        scan.saw_assistant = true;
        let text = message_text(item);
        if let Some(completed) = value_field_i64(item, &["time", "completed"]) {
            if scan.completed_at_ms.is_none() {
                scan.completed_at_ms = Some(completed);
            }
        }
        if !text.trim().is_empty() {
            scan.text = Some(text.trim().to_string());
            return scan;
        }
    }
    scan
}

/// 看门狗判定（纯函数，便于测试）。
pub fn watchdog_decision(
    dispatched_at_ms: Option<i64>,
    settled_turn_ms: Option<i64>,
    turn: &TurnScan,
    grace_ms: i64,
    now_ms: i64,
) -> WatchdogDecision {
    // 旧任务/未记录派活时刻：不猜（由 `run_once` 写基线后跳过）。
    let Some(dispatched_at_ms) = dispatched_at_ms else {
        return WatchdogDecision::None;
    };
    if !turn.saw_assistant {
        return WatchdogDecision::None;
    }
    if !turn.closed {
        // 回合还在进行中（会话尾部没有 idle 标记）：不处理，避免把进行中的旁白当成最终汇报。
        return WatchdogDecision::None;
    }
    let Some(completed_at_ms) = turn.completed_at_ms else {
        return WatchdogDecision::None;
    };
    if completed_at_ms <= dispatched_at_ms {
        return WatchdogDecision::None;
    }
    if settled_turn_ms.is_some_and(|settled| completed_at_ms <= settled) {
        return WatchdogDecision::None;
    }
    if completed_at_ms.saturating_add(grace_ms) > now_ms {
        return WatchdogDecision::None;
    }
    match turn.text.as_deref() {
        Some(body) if !body.trim().is_empty() => WatchdogDecision::Report {
            body: body.to_string(),
            completed_at_ms,
            failed: false,
        },
        // 回合确实有 assistant 响应但没有任何文字：按失败回注（阻塞 + 可读建议）。
        _ => WatchdogDecision::Report {
            body: format!("任务执行失败：{NO_REPORT_FAILURE}"),
            completed_at_ms,
            failed: true,
        },
    }
}

/// 会话探针：解析真实会话 id + 读取消息（可注入假实现做测试）。
#[async_trait::async_trait]
pub trait OrcSessionProbe: Send + Sync {
    /// 解析任务某步骤的真实 OpenCode 会话 id；查不到返回 `Ok(None)`（不猜）。
    async fn resolve_session(&self, logical_id: &str) -> Result<Option<String>, String>;
    /// 会话消息（原始 JSON 项；顺序不做假设）。
    async fn session_messages(&self, session_id: &str) -> Result<Vec<Value>, String>;
}

/// 生产探针：读会话映射文件 + OpenCode 只读 HTTP API（Basic 认证同模型列表）。
pub struct OpenCodeSessionProbe {
    password: String,
    port: u16,
    client: reqwest::Client,
    session_map_path: PathBuf,
}

/// 会话列表项（标题匹配兜底用）。
#[derive(Debug, Clone)]
struct ProbeSession {
    id: String,
    title: String,
}

impl OpenCodeSessionProbe {
    /// 读取服务密码与端口；失败返回可读原因（调用方记日志并跳过本轮）。
    pub fn new() -> Result<Self, String> {
        let home = std::env::var_os("USERPROFILE")
            .or_else(|| std::env::var_os("HOME"))
            .ok_or_else(|| "无法确定用户目录".to_string())?;
        let home = PathBuf::from(home);
        let password = super::opencode_models::read_service_password(
            &home.join(super::opencode_models::OPENCODE_SERVICE_RELATIVE),
        )
        .map_err(|error| error.message().to_string())?;
        let port = super::opencode_models::port_from_latest_log(
            &home.join(super::opencode_models::OPENCODE_LOGS_RELATIVE),
        )
        .unwrap_or(super::opencode_models::OPENCODE_DEFAULT_PORT);
        let client = reqwest::Client::builder()
            .timeout(REQUEST_TIMEOUT)
            .build()
            .map_err(|error| format!("初始化 OpenCode 请求客户端失败：{error}"))?;
        let reply_dir = std::env::var_os("AGENT_NOTIFY_OPENCODE_REPLY_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join(REPLY_DIR_RELATIVE));
        Ok(Self {
            password,
            port,
            client,
            session_map_path: reply_dir.join(SESSION_MAP_FILE),
        })
    }

    fn get(&self, path: &str) -> reqwest::RequestBuilder {
        self.client
            .get(format!("http://127.0.0.1:{}{path}", self.port))
            .basic_auth("opencode", Some(&self.password))
    }

    /// 会话映射文件里的真实 id（缺失/损坏返回 None，由调用方按标题兜底）。
    fn session_from_map(&self, logical_id: &str) -> Option<String> {
        let raw = std::fs::read_to_string(&self.session_map_path).ok()?;
        let parsed: Value = serde_json::from_str(&raw).ok()?;
        let id = parsed.get(logical_id)?.get("id")?.as_str()?.trim();
        (!id.is_empty()).then(|| id.to_string())
    }

    /// 会话列表（标题匹配兜底用）。
    async fn sessions(&self) -> Result<Vec<ProbeSession>, String> {
        let response = self
            .get("/api/session")
            .send()
            .await
            .map_err(|error| format!("连接 OpenCode 会话列表失败：{error}"))?;
        if !response.status().is_success() {
            return Err(format!("OpenCode 会话列表返回 HTTP {}", response.status()));
        }
        let payload: Value = response
            .json()
            .await
            .map_err(|error| format!("解析 OpenCode 会话列表失败：{error}"))?;
        let items = json_items(&payload);
        let sessions: Vec<ProbeSession> = items
            .iter()
            .filter_map(|item| {
                let id = item.get("id").and_then(Value::as_str)?.trim().to_string();
                if id.is_empty() {
                    return None;
                }
                let title = item
                    .get("title")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string();
                Some(ProbeSession { id, title })
            })
            .collect();
        Ok(sessions)
    }
}

#[async_trait::async_trait]
impl OrcSessionProbe for OpenCodeSessionProbe {
    async fn resolve_session(&self, logical_id: &str) -> Result<Option<String>, String> {
        if let Some(id) = self.session_from_map(logical_id) {
            return Ok(Some(id));
        }
        // 兜底：按标题里的逻辑会话 id 匹配（旧任务/映射文件缺失）。
        let sessions = self.sessions().await?;
        let found = sessions
            .iter()
            .filter(|session| session.title.contains(logical_id))
            .max_by_key(|session| session.title.len());
        Ok(found.map(|session| session.id.clone()))
    }

    async fn session_messages(&self, session_id: &str) -> Result<Vec<Value>, String> {
        let response = self
            .get(&format!("/api/session/{session_id}/message"))
            .send()
            .await
            .map_err(|error| format!("连接 OpenCode 会话消息失败：{error}"))?;
        if !response.status().is_success() {
            return Err(format!("OpenCode 会话消息返回 HTTP {}", response.status()));
        }
        let payload: Value = response
            .json()
            .await
            .map_err(|error| format!("解析 OpenCode 会话消息失败：{error}"))?;
        Ok(json_items(&payload)
            .into_iter()
            .cloned()
            .collect::<Vec<Value>>())
    }
}

/// 从各种包装形状里取消息/会话数组（`{data: [...]}`、`{data: {items: [...]}}`、裸数组）。
fn json_items(payload: &Value) -> Vec<&Value> {
    if let Some(array) = payload.as_array() {
        return array.iter().collect();
    }
    let Some(data) = payload.get("data") else {
        return Vec::new();
    };
    if let Some(array) = data.as_array() {
        return array.iter().collect();
    }
    data.get("items")
        .and_then(Value::as_array)
        .map(|array| array.iter().collect())
        .unwrap_or_default()
}

/// 消息类型：`info.type/role` 或顶层 `type/role`。
fn message_kind(item: &Value) -> Option<String> {
    let info = item.get("info").filter(|value| value.is_object());
    for source in [info.unwrap_or(item), item] {
        for key in ["type", "role"] {
            if let Some(value) = source.get(key).and_then(Value::as_str) {
                if !value.trim().is_empty() {
                    return Some(value.trim().to_string());
                }
            }
        }
    }
    None
}

/// 消息正文：`content`/`parts` 里所有 `type == "text"` 片段拼接。
fn message_text(item: &Value) -> String {
    let parts = item
        .get("content")
        .or_else(|| item.get("parts"))
        .and_then(Value::as_array);
    let mut chunks = Vec::new();
    for part in parts.into_iter().flatten() {
        if part.get("type").and_then(Value::as_str) != Some("text") {
            continue;
        }
        if let Some(text) = part.get("text").and_then(Value::as_str) {
            if !text.trim().is_empty() {
                chunks.push(text.trim().to_string());
            }
        }
    }
    chunks.join("\n")
}

/// 最后一条 user 消息的创建时间（旧任务看门狗基线的依据；没有 user 消息返回 None）。
fn last_user_created_ms(items: &[Value]) -> Option<i64> {
    let mut ordered: Vec<&Value> = items.iter().collect();
    ordered.sort_by_key(|item| value_field_i64(item, &["time", "created"]).unwrap_or(0));
    ordered
        .iter()
        .rev()
        .find(|item| message_kind(item).as_deref() == Some("user"))
        .and_then(|item| value_field_i64(item, &["time", "created"]))
}

/// 读取 `time.created` 之类的嵌套数值字段（同时兼容数字与数字字符串）。
fn value_field_i64(item: &Value, path: &[&str]) -> Option<i64> {
    let info = item.get("info").filter(|value| value.is_object());
    for source in [info.unwrap_or(item), item] {
        let mut current = source;
        let mut ok = true;
        for key in path {
            match current.get(*key) {
                Some(next) => current = next,
                None => {
                    ok = false;
                    break;
                }
            }
        }
        if !ok {
            continue;
        }
        if let Some(value) = current.as_i64() {
            return Some(value);
        }
        if let Some(text) = current.as_str() {
            if let Ok(value) = text.parse::<i64>() {
                return Some(value);
            }
        }
    }
    None
}

/// 看门狗：定期核对运行中的任务并按需回注汇报。
pub struct OrcWatchdog {
    handler: Arc<OrcCommandHandler>,
    repository: Arc<dyn OrcTaskRepository>,
    probe: Arc<dyn OrcSessionProbe>,
}

impl OrcWatchdog {
    pub fn new(
        handler: Arc<OrcCommandHandler>,
        repository: Arc<dyn OrcTaskRepository>,
        probe: Arc<dyn OrcSessionProbe>,
    ) -> Self {
        Self {
            handler,
            repository,
            probe,
        }
    }

    /// 进程内只允许一个巡检循环（生产装配可能被重入）。
    fn claim_spawn_slot() -> bool {
        static SPAWNED: AtomicBool = AtomicBool::new(false);
        SPAWNED
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_ok()
    }

    /// 启动巡检循环（进程内单例；随进程退出结束）。
    pub fn spawn(self: Arc<Self>) {
        if !Self::claim_spawn_slot() {
            return;
        }
        tracing::info!(
            interval_secs = WATCHDOG_INTERVAL.as_secs(),
            "编排看门狗已启动：定期核对漏报的 Agent 汇报"
        );
        tokio::spawn(async move {
            loop {
                let now_ms = Timestamp::now_utc().unix_millis();
                let recovered = self.run_once(now_ms).await;
                if recovered > 0 {
                    tracing::info!(recovered, "看门狗回注了漏掉的 Agent 汇报");
                }
                tokio::time::sleep(WATCHDOG_INTERVAL).await;
            }
        });
    }

    /// 跑一轮巡检，返回回注次数（测试可直接调用）。
    pub async fn run_once(&self, now_ms: i64) -> usize {
        let tasks = match self.repository.list_tasks().await {
            Ok(tasks) => tasks,
            Err(error) => {
                tracing::warn!(
                    code = error.code(),
                    "看门狗读取任务列表失败：{}",
                    error.message()
                );
                return 0;
            }
        };
        let mut recovered = 0;
        for a2a in tasks {
            let Ok(mut task) = OrcTask::from_a2a(a2a) else {
                continue;
            };
            if !is_watchable(&task) {
                continue;
            }
            let Ok(meta) = task.meta() else {
                continue;
            };
            let step = meta.current_step;
            let logical_id = format!("task-{}-step-{}", task_id_of(&task), step);
            let session_id = match self.probe.resolve_session(&logical_id).await {
                Ok(Some(id)) => id,
                Ok(None) => {
                    tracing::debug!(logical_id, "看门狗未找到会话（可能尚未派活），跳过");
                    continue;
                }
                Err(error) => {
                    tracing::warn!(logical_id, "看门狗解析会话失败：{error}");
                    continue;
                }
            };
            let messages = match self.probe.session_messages(&session_id).await {
                Ok(messages) => messages,
                Err(error) => {
                    tracing::warn!(logical_id, "看门狗读取会话消息失败：{error}");
                    continue;
                }
            };
            let mut dispatched_at_ms = meta.last_dispatch_at_ms;
            if dispatched_at_ms.is_none() {
                // 旧任务（升级前没有派活时刻）：用会话里最后一条 user 消息时间当基线
                // （本次派活的信封就是最后一条 user），先写基线再按同一套判定兜底当前回合；
                // 取不到会话消息时写 now 并跳过，不猜历史回合。
                let baseline = last_user_created_ms(&messages);
                let value = baseline.unwrap_or(now_ms);
                let _ = task.mark_dispatched(value);
                let _ = self.repository.save_task(&task.a2a_task).await;
                dispatched_at_ms = Some(value);
                if baseline.is_none() {
                    tracing::info!(
                        task_id = task_id_of(&task),
                        step,
                        "看门狗为旧任务写入派活基线（无会话消息，本轮跳过）"
                    );
                    continue;
                }
                tracing::info!(
                    task_id = task_id_of(&task),
                    step,
                    baseline_ms = value,
                    "看门狗为旧任务按最后一条消息写入派活基线"
                );
            }
            let turn = scan_turn(&messages);
            let WatchdogDecision::Report {
                body,
                completed_at_ms,
                failed,
            } = watchdog_decision(
                dispatched_at_ms,
                meta.last_settled_turn_ms,
                &turn,
                WATCHDOG_GRACE_MS,
                now_ms,
            )
            else {
                continue;
            };
            match self
                .handler
                .report_from_agent(&task_id_of(&task), step, &body, failed)
                .await
            {
                Ok(true) => {
                    recovered += 1;
                    let mut settled = match self.repository.get_task(&task_id_of(&task)).await {
                        Ok(Some(a2a)) => OrcTask::from_a2a(a2a).ok(),
                        _ => None,
                    };
                    if let Some(task) = settled.as_mut() {
                        let _ = task.mark_settled_turn(completed_at_ms);
                        let _ = self.repository.save_task(&task.a2a_task).await;
                    }
                    tracing::info!(
                        task_id = task_id_of(&task),
                        step,
                        failed,
                        "看门狗回注漏掉的 Agent 汇报，任务自动推进"
                    );
                }
                Ok(false) => {
                    tracing::debug!(
                        task_id = task_id_of(&task),
                        step,
                        "看门狗候选汇报与任务当前状态不匹配，已忽略"
                    );
                }
                Err(error) => {
                    tracing::warn!(
                        task_id = task_id_of(&task),
                        step,
                        code = error.code(),
                        "看门狗回注失败：{}",
                        error.message()
                    );
                }
            }
        }
        recovered
    }
}

/// 只处理「已开始、干活中、未汇总、未阻塞」的任务。
fn is_watchable(task: &OrcTask) -> bool {
    let Ok(meta) = task.meta() else {
        return false;
    };
    meta.started
        && !meta.final_report_pending
        && meta.blocked_step.is_none()
        && task.state() == TaskState::Working
}

fn task_id_of(task: &OrcTask) -> String {
    task.id().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn message(kind: &str, created: i64, text: Option<&str>) -> Value {
        let mut parts = Vec::new();
        if let Some(value) = text {
            parts.push(json!({"type": "text", "text": value}));
        }
        json!({
            "info": {"type": kind, "time": {"created": created, "completed": created}},
            "parts": parts,
        })
    }

    /// 本回合（最后一条 user 之后）最新带正文的 assistant 产出。
    #[test]
    fn scan_turn_takes_newest_text_in_turn() {
        let items = vec![
            message("assistant", 100, Some("上一轮正文")),
            message("user", 200, None),
            message("assistant", 300, None),
            message("assistant", 400, Some("本回合正文")),
            json!({"info": {"type": "idle", "time": {"created": 500}}, "parts": []}),
        ];
        let turn = scan_turn(&items);
        assert!(turn.saw_assistant);
        assert!(turn.closed, "尾部有 idle 标记 = 回合已收束");
        assert_eq!(turn.text.as_deref(), Some("本回合正文"));
        assert_eq!(turn.completed_at_ms, Some(400));
    }

    /// 回合进行中（尾部仍是 assistant，没有 idle 标记）：不得视为已收束。
    #[test]
    fn scan_turn_marks_open_turn_as_unclosed() {
        let items = vec![
            message("user", 200, None),
            message("assistant", 300, Some("进行中的旁白")),
            message("assistant", 400, Some("还在跑：工具调用之后的新旁白")),
        ];
        let turn = scan_turn(&items);
        assert!(turn.saw_assistant);
        assert!(!turn.closed, "进行中的回合不得标记为已收束");
        // 未收束的回合一律不回注（真机事故：把旁白当成最终汇报）。
        assert_eq!(
            watchdog_decision(Some(100), None, &turn, 60_000, 100_000),
            WatchdogDecision::None
        );
    }

    /// 回合里没有正文：`text` 为空但 `saw_assistant` 为真（失败回注用）。
    #[test]
    fn scan_turn_without_text_marks_assistant_seen() {
        let items = vec![
            message("assistant", 100, Some("上一轮正文")),
            message("user", 200, None),
            message("assistant", 300, None),
            json!({"info": {"type": "idle", "time": {"created": 350}}, "parts": []}),
        ];
        let turn = scan_turn(&items);
        assert!(turn.saw_assistant);
        assert!(turn.closed);
        assert_eq!(turn.text, None);
        assert_eq!(turn.completed_at_ms, Some(300));
    }

    /// 判定：产出必须晚于派活、超出宽限、回合已收束、且未回注过。
    #[test]
    fn watchdog_decision_guards() {
        let turn = TurnScan {
            saw_assistant: true,
            closed: true,
            text: Some("完成正文".into()),
            completed_at_ms: Some(10_000),
        };
        // 正常：派活 5s、产出 10s、已过宽限。
        assert_eq!(
            watchdog_decision(Some(5_000), None, &turn, 60_000, 100_000),
            WatchdogDecision::Report {
                body: "完成正文".into(),
                completed_at_ms: 10_000,
                failed: false,
            }
        );
        // 没有派活时刻（旧任务）：不猜。
        assert_eq!(
            watchdog_decision(None, None, &turn, 60_000, 100_000),
            WatchdogDecision::None
        );
        // 产出早于/等于派活：是上一回合，不接受。
        assert_eq!(
            watchdog_decision(Some(10_000), None, &turn, 60_000, 100_000),
            WatchdogDecision::None
        );
        // 还在宽限期：先等插件。
        assert_eq!(
            watchdog_decision(Some(5_000), None, &turn, 60_000, 30_000),
            WatchdogDecision::None
        );
        // 已回注过同一回合：不重复。
        assert_eq!(
            watchdog_decision(Some(5_000), Some(10_000), &turn, 60_000, 100_000),
            WatchdogDecision::None
        );
    }

    /// 判定：回合有响应但无正文 → 按失败回注（阻塞 + 可读建议）。
    #[test]
    fn watchdog_decision_reports_empty_turn_as_failure() {
        let turn = TurnScan {
            saw_assistant: true,
            closed: true,
            text: None,
            completed_at_ms: Some(10_000),
        };
        match watchdog_decision(Some(5_000), None, &turn, 60_000, 100_000) {
            WatchdogDecision::Report { body, failed, .. } => {
                assert!(failed);
                assert!(body.contains("没有产出汇报"), "{body}");
            }
            other => panic!("必须按失败回注：{other:?}"),
        }
    }

    /// 消息形状兼容：`type` 顶层 + `content` 片段。
    #[test]
    fn scan_turn_supports_v2_message_shape() {
        let items = vec![
            json!({"id": "u1", "type": "user", "time": {"created": 10}, "content": []}),
            json!({
                "id": "a1",
                "type": "assistant",
                "time": {"created": 20, "completed": 25},
                "content": [{"type": "text", "text": "V2 正文"}],
            }),
        ];
        let turn = scan_turn(&items);
        assert_eq!(turn.text.as_deref(), Some("V2 正文"));
        assert_eq!(turn.completed_at_ms, Some(25));
        assert!(!turn.closed, "没有 idle 标记 = 回合未收束");
    }
}
