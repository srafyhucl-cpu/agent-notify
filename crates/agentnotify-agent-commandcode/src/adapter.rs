use std::time::Duration;

use agentnotify_agent_sdk::{
    AgentAdapter, AgentCapabilities, AgentDescriptor, AgentError, AgentEventEnvelope, AgentHealth,
    NormalizedAgentEvent, ResumeReceipt,
};
use agentnotify_domain::{AgentSessionId, SafeError};

use crate::{
    descriptor::{capabilities, descriptor},
    event::parse_event,
    reply_inbox::{CommandCodeReplyInbox, DEFAULT_RESULT_WAIT, health_for},
    title::CommandCodeSessions,
    window::{clamp_reply_window_sec, resolve_reply_window_sec},
};

/// Command Code 适配器：事件标准化、本地会话标题解析与受回复窗口约束的会话注入。
#[derive(Clone)]
pub struct CommandCodeAgent {
    sessions: CommandCodeSessions,
    inbox: CommandCodeReplyInbox,
    reply_window_sec: u64,
}

impl CommandCodeAgent {
    /// 显式数据源构造；测试与自定义安装位置用。
    pub fn new(
        sessions: CommandCodeSessions,
        inbox: CommandCodeReplyInbox,
        reply_window_sec: u64,
    ) -> Self {
        Self {
            sessions,
            inbox,
            reply_window_sec: clamp_reply_window_sec(reply_window_sec),
        }
    }

    /// 按环境覆盖与默认安装位置构造；窗口秒数默认 0（关闭）。
    pub fn from_default_location() -> Self {
        Self::new(
            CommandCodeSessions::from_default_location(),
            CommandCodeReplyInbox::from_default_location(),
            resolve_reply_window_sec(),
        )
    }

    /// 不读取任何用户目录、也不接触真实 Command Code 数据的测试实例。
    pub fn new_test(reply_window_sec: u64) -> Self {
        Self::new(
            CommandCodeSessions::without_backend(),
            CommandCodeReplyInbox::without_backend(),
            reply_window_sec,
        )
    }

    pub fn with_sessions(mut self, sessions: CommandCodeSessions) -> Self {
        self.sessions = sessions;
        self
    }

    pub fn with_inbox(mut self, inbox: CommandCodeReplyInbox) -> Self {
        self.inbox = inbox;
        self
    }

    /// 覆盖回复窗口秒数；界面里配置的 `commandCodeReplyWindowSec` 生效。
    pub fn with_reply_window_sec(mut self, reply_window_sec: u64) -> Self {
        self.reply_window_sec = clamp_reply_window_sec(reply_window_sec);
        self
    }

    pub fn sessions(&self) -> &CommandCodeSessions {
        &self.sessions
    }

    pub fn inbox(&self) -> &CommandCodeReplyInbox {
        &self.inbox
    }

    pub fn reply_window_sec(&self) -> u64 {
        self.reply_window_sec
    }

    pub async fn resume_with_timeout(
        &self,
        session_id: &AgentSessionId,
        text: &str,
        timeout: Duration,
    ) -> Result<ResumeReceipt, AgentError> {
        if text.trim().is_empty() {
            return Err(AgentError::InvalidInput);
        }
        self.ensure_reply_window_open()?;
        self.inbox
            .resume_with_timeout(session_id, text, timeout)
            .await?;
        Ok(ResumeReceipt {
            session_id: session_id.clone(),
        })
    }

    /// 窗口配置为 0 时明确失败：不写任务、不挂住 run、不回退到最近会话。
    fn ensure_reply_window_open(&self) -> Result<(), AgentError> {
        if self.reply_window_sec > 0 {
            return Ok(());
        }
        Err(failed(
            "commandcode_reply_window_closed",
            "Command Code 回复窗口未开启：请在 AgentNotify 设置里把 CommandCode 回复窗口设为 1–600 秒并重启 Command Code，再引用本条通知回复",
        ))
    }
}

#[async_trait::async_trait]
impl AgentAdapter for CommandCodeAgent {
    fn descriptor(&self) -> AgentDescriptor {
        descriptor()
    }

    fn capabilities(&self) -> AgentCapabilities {
        capabilities()
    }

    fn parse_event(
        &self,
        envelope: AgentEventEnvelope,
    ) -> Result<NormalizedAgentEvent, AgentError> {
        parse_event(envelope, &self.sessions)
    }

    async fn resume(
        &self,
        session_id: &AgentSessionId,
        text: &str,
    ) -> Result<ResumeReceipt, AgentError> {
        self.resume_with_timeout(session_id, text, DEFAULT_RESULT_WAIT)
            .await
    }

    async fn inspect(&self) -> AgentHealth {
        health_for(self.inbox.inspect_mod_state().await)
    }
}

fn failed(code: &str, message: &str) -> AgentError {
    AgentError::Failed(safe_error(code, message))
}

fn safe_error(code: &str, message: &str) -> SafeError {
    SafeError::new(code, message).expect("Command Code 错误常量必须是有效安全错误")
}
