use std::{fmt::Display, sync::Arc};

use agentnotify_agent_sdk::{AgentError, AgentEventEnvelope, AgentRegistry};
use agentnotify_domain::{AgentId, DomainArea, Notification, NotificationId, NotificationMetadata};

use crate::{Clock, EventSink, IdGenerator, IngestStore, OutboxItem, StoreError, UseCase};

use crate::policy::{NotificationPolicy, PolicyDecision, PolicyInput, SkipReason};

/// 入站处理结果，不包含正文或渠道凭据。
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum IngestResult {
    Queued { notification_id: NotificationId },
    Duplicate { notification_id: NotificationId },
    Skipped { reason: SkipReason },
}

impl IngestResult {
    pub fn notification_id(&self) -> Option<&NotificationId> {
        match self {
            Self::Queued { notification_id } | Self::Duplicate { notification_id } => {
                Some(notification_id)
            }
            Self::Skipped { .. } => None,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum IngestError {
    AgentNotRegistered { agent_id: AgentId },
    Agent(AgentError),
    Store(StoreError),
    InvalidNotification,
}

impl IngestError {
    pub fn code(&self) -> &str {
        match self {
            Self::AgentNotRegistered { .. } => "agent_not_registered",
            Self::Agent(error) => error.code(),
            Self::Store(error) => error.code(),
            Self::InvalidNotification => "invalid_notification",
        }
    }

    pub fn message(&self) -> &str {
        match self {
            Self::AgentNotRegistered { .. } => "Agent 未注册，无法处理该事件",
            Self::Agent(error) => error.message(),
            Self::Store(error) => error.message(),
            Self::InvalidNotification => "Agent 事件无法转换为通知",
        }
    }
}

impl Display for IngestError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.message())
    }
}

impl std::error::Error for IngestError {}

impl From<AgentError> for IngestError {
    fn from(value: AgentError) -> Self {
        Self::Agent(value)
    }
}

impl From<StoreError> for IngestError {
    fn from(value: StoreError) -> Self {
        Self::Store(value)
    }
}

/// 将 Agent 事件转换为通知和 Outbox，不访问渠道网络。
#[derive(Clone)]
pub struct IngestService {
    agents: Arc<AgentRegistry>,
    store: Arc<dyn IngestStore>,
    event_sink: Arc<dyn EventSink>,
    clock: Arc<dyn Clock>,
    id_generator: Arc<dyn IdGenerator>,
    policy: NotificationPolicy,
}

impl IngestService {
    pub fn new(
        agents: Arc<AgentRegistry>,
        store: Arc<dyn IngestStore>,
        event_sink: Arc<dyn EventSink>,
        clock: Arc<dyn Clock>,
        id_generator: Arc<dyn IdGenerator>,
        policy: NotificationPolicy,
    ) -> Self {
        Self {
            agents,
            store,
            event_sink,
            clock,
            id_generator,
            policy,
        }
    }

    pub async fn ingest(&self, envelope: AgentEventEnvelope) -> Result<IngestResult, IngestError> {
        let agent_id = envelope.agent_id.clone();
        let request_id = envelope.request_id.clone();
        let adapter =
            self.agents
                .get(&agent_id)
                .ok_or_else(|| IngestError::AgentNotRegistered {
                    agent_id: agent_id.clone(),
                })?;
        let event = adapter.parse_event(envelope)?;
        let ingest_key = event
            .idempotency_key
            .clone()
            .unwrap_or_else(|| request_id.to_string());

        if let Some(existing) = self
            .store
            .notification_by_ingest_key(&agent_id, &ingest_key)
            .await?
        {
            return Ok(IngestResult::Duplicate {
                notification_id: existing.id,
            });
        }

        let now = self.clock.now();
        let recent_notification_at = match event.session_id.as_ref() {
            Some(session_id) => {
                self.store
                    .recent_notification_at(&agent_id, session_id)
                    .await?
            }
            None => None,
        };
        let decision = self.policy.evaluate(&PolicyInput {
            agent_id: &agent_id,
            session_id: event.session_id.as_ref(),
            title: &event.title,
            now,
            recent_notification_at,
        });

        let notification_id = NotificationId::new(self.id_generator.next_id())
            .map_err(|_| IngestError::InvalidNotification)?;
        let metadata = match decision {
            PolicyDecision::Deliver => event.metadata.clone(),
            PolicyDecision::Skip(reason) => {
                with_skip_reason(&event.metadata, reason).ok_or(IngestError::InvalidNotification)?
            }
        };
        let notification = Notification::new(
            notification_id.clone(),
            ingest_key,
            agent_id.clone(),
            event.session_id,
            event.session_title,
            event.title,
            event.body,
            event.occurred_at,
            metadata,
        )
        .map_err(|_| IngestError::InvalidNotification)?;

        let result = match decision {
            PolicyDecision::Deliver => {
                let outbox = OutboxItem::pending(
                    format!("outbox-{}", notification.id),
                    notification.id.clone(),
                    now,
                );
                match self
                    .store
                    .commit_ingest(notification.clone(), vec![outbox])
                    .await
                {
                    Ok(()) => IngestResult::Queued {
                        notification_id: notification.id.clone(),
                    },
                    Err(error) if error.code() == "store_conflict" => {
                        if let Some(existing) = self
                            .store
                            .notification_by_ingest_key(&agent_id, &notification.ingest_key)
                            .await?
                        {
                            return Ok(IngestResult::Duplicate {
                                notification_id: existing.id,
                            });
                        }
                        return Err(IngestError::Store(error));
                    }
                    Err(error) => return Err(IngestError::Store(error)),
                }
            }
            PolicyDecision::Skip(reason) => {
                self.store
                    .commit_ingest(notification.clone(), Vec::new())
                    .await?;
                IngestResult::Skipped { reason }
            }
        };

        self.event_sink.notification_changed(&notification.id).await;
        Ok(result)
    }
}

impl UseCase for IngestService {
    fn area(&self) -> DomainArea {
        DomainArea::Ingest
    }

    fn name(&self) -> &'static str {
        "ingest"
    }
}

fn with_skip_reason(
    metadata: &NotificationMetadata,
    reason: SkipReason,
) -> Option<NotificationMetadata> {
    let mut values = metadata.clone().into_inner();
    values.insert("skip_reason".into(), reason.as_str().into());
    NotificationMetadata::new(values).ok()
}
