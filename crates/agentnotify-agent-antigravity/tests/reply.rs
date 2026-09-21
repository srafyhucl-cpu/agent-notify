//! 精确会话回复：只在认识目标会话的本机端点上发送；
//! 找不到语言服务或会话时明确失败，绝不新建服务，也绝不回退到最近会话。

use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::Duration,
};

use agentnotify_agent_antigravity::{
    AgentApi, AntigravityAgent, AntigravityReply, EndpointDiscovery, LanguageServerEndpoint,
    antigravity_resume_target,
};
use agentnotify_agent_sdk::{AgentAdapter, AgentError};
use agentnotify_domain::{AgentSessionId, SafeError};

#[derive(Clone, Default)]
struct FakeDiscovery {
    endpoints: Vec<LanguageServerEndpoint>,
    error: Option<AgentError>,
    calls: Arc<Mutex<usize>>,
}

impl FakeDiscovery {
    fn calls(&self) -> usize {
        *self.calls.lock().unwrap()
    }
}

impl EndpointDiscovery for FakeDiscovery {
    fn discover(&self) -> Result<Vec<LanguageServerEndpoint>, AgentError> {
        *self.calls.lock().unwrap() += 1;
        match &self.error {
            Some(error) => Err(error.clone()),
            None => Ok(self.endpoints.clone()),
        }
    }
}

#[derive(Clone, Default)]
struct FakeApi {
    /// conversationId → 认识它的端点地址；未列出的会话在任何端点上都不存在。
    exists: HashMap<String, String>,
    exists_error: Option<AgentError>,
    /// 该地址的 metadata 探测会挂住，用来覆盖探测超时后换下一个端点的路径。
    slow_metadata_address: Option<String>,
    send_error: Option<AgentError>,
    send_delay: Option<Duration>,
    probes: Arc<Mutex<Vec<String>>>,
    sent: Arc<Mutex<Vec<(String, String, String)>>>,
}

impl FakeApi {
    fn probes(&self) -> Vec<String> {
        self.probes.lock().unwrap().clone()
    }

    fn sent(&self) -> Vec<(String, String, String)> {
        self.sent.lock().unwrap().clone()
    }
}

#[async_trait::async_trait]
impl AgentApi for FakeApi {
    async fn conversation_exists(
        &self,
        endpoint: &LanguageServerEndpoint,
        conversation_id: &str,
    ) -> Result<bool, AgentError> {
        self.probes
            .lock()
            .unwrap()
            .push(format!("{}|{conversation_id}", endpoint.address));
        if self.slow_metadata_address.as_deref() == Some(endpoint.address.as_str()) {
            tokio::time::sleep(Duration::from_secs(5)).await;
        }
        if let Some(error) = &self.exists_error {
            return Err(error.clone());
        }
        Ok(self.exists.get(conversation_id) == Some(&endpoint.address))
    }

    async fn send_message(
        &self,
        endpoint: &LanguageServerEndpoint,
        conversation_id: &str,
        text: &str,
    ) -> Result<(), AgentError> {
        self.sent.lock().unwrap().push((
            endpoint.address.clone(),
            conversation_id.to_owned(),
            text.to_owned(),
        ));
        if let Some(delay) = self.send_delay {
            tokio::time::sleep(delay).await;
        }
        match &self.send_error {
            Some(error) => Err(error.clone()),
            None => Ok(()),
        }
    }
}

fn endpoint(port: u16) -> LanguageServerEndpoint {
    LanguageServerEndpoint::loopback(port, "csrf-token")
}

fn reply(discovery: FakeDiscovery, api: FakeApi) -> AntigravityReply {
    AntigravityReply::new(Arc::new(discovery), Arc::new(api))
}

fn agent(discovery: FakeDiscovery, api: FakeApi) -> AntigravityAgent {
    AntigravityAgent::new_test().with_reply(reply(discovery, api))
}

fn session(id: &str) -> AgentSessionId {
    AgentSessionId::new(id).unwrap()
}

fn safe_error(code: &str, message: &str) -> SafeError {
    SafeError::new(code, message).unwrap()
}

#[test]
fn conversation_id_is_the_only_resume_target() {
    assert_eq!(
        antigravity_resume_target("conversation-1").as_str(),
        "conversation-1"
    );
}

#[test]
fn endpoints_require_loopback_address_and_token() {
    assert!(LanguageServerEndpoint::loopback(62957, "csrf-token").is_loopback());
    for endpoint in [
        LanguageServerEndpoint {
            address: "0.0.0.0:62957".into(),
            token: "csrf-token".into(),
        },
        LanguageServerEndpoint {
            address: "192.168.1.10:62957".into(),
            token: "csrf-token".into(),
        },
        LanguageServerEndpoint {
            address: "127.0.0.1:0".into(),
            token: "csrf-token".into(),
        },
        LanguageServerEndpoint {
            address: "127.0.0.1:62957".into(),
            token: "   ".into(),
        },
    ] {
        assert!(!endpoint.is_loopback(), "{endpoint:?}");
    }
}

#[tokio::test]
async fn resume_sends_to_the_exact_conversation_on_the_matching_endpoint() {
    let api = FakeApi {
        exists: HashMap::from([("conversation-1".to_owned(), "127.0.0.1:2".to_owned())]),
        ..FakeApi::default()
    };
    let adapter = agent(
        FakeDiscovery {
            endpoints: vec![endpoint(1), endpoint(2)],
            ..FakeDiscovery::default()
        },
        api.clone(),
    );

    let receipt = adapter
        .resume(&session("conversation-1"), " 继续检查 ")
        .await
        .unwrap();

    assert_eq!(receipt.session_id, session("conversation-1"));
    assert_eq!(
        api.sent(),
        vec![(
            "127.0.0.1:2".to_owned(),
            "conversation-1".to_owned(),
            "继续检查".to_owned()
        )]
    );
    assert_eq!(
        api.probes(),
        vec![
            "127.0.0.1:1|conversation-1".to_owned(),
            "127.0.0.1:2|conversation-1".to_owned()
        ],
        "必须逐个端点验证目标会话，只有认识它的端点才会收到消息"
    );
}

#[tokio::test]
async fn unknown_conversation_is_unavailable_and_never_sends() {
    let api = FakeApi {
        exists: HashMap::from([("conversation-1".to_owned(), "127.0.0.1:1".to_owned())]),
        ..FakeApi::default()
    };
    let adapter = agent(
        FakeDiscovery {
            endpoints: vec![endpoint(1)],
            ..FakeDiscovery::default()
        },
        api.clone(),
    );

    let error = adapter
        .resume(&session("conversation-2"), "继续")
        .await
        .unwrap_err();

    assert_eq!(error.code(), "antigravity_conversation_unavailable");
    assert!(error.to_string().contains("仍存在于桌面端"), "{error}");
    assert!(api.sent().is_empty(), "目标会话不存在时不得发送");
    assert_eq!(
        api.probes(),
        vec!["127.0.0.1:1|conversation-2".to_owned()],
        "不得用其他会话顶替"
    );
}

#[tokio::test]
async fn missing_language_server_is_unavailable_without_spawning() {
    let api = FakeApi::default();
    let discovery = FakeDiscovery::default();
    let adapter = agent(discovery.clone(), api.clone());

    let error = adapter
        .resume(&session("conversation-1"), "继续")
        .await
        .unwrap_err();

    assert_eq!(error.code(), "antigravity_language_server_missing");
    assert!(
        error
            .to_string()
            .contains("请确认 Antigravity 桌面端已打开"),
        "{error}"
    );
    assert_eq!(discovery.calls(), 1, "解析失败不得重试");
    assert!(api.sent().is_empty());
}

#[tokio::test]
async fn discovery_failure_is_exposed_as_is() {
    let api = FakeApi::default();
    let adapter = agent(
        FakeDiscovery {
            error: Some(AgentError::Unavailable(safe_error(
                "antigravity_language_server_path_invalid",
                "Antigravity 语言服务路径不可用",
            ))),
            ..FakeDiscovery::default()
        },
        api.clone(),
    );

    let error = adapter
        .resume(&session("conversation-1"), "继续")
        .await
        .unwrap_err();

    assert_eq!(error.code(), "antigravity_language_server_path_invalid");
    assert!(api.sent().is_empty());
}

#[tokio::test]
async fn endpoints_outside_loopback_are_rejected_before_any_call() {
    let api = FakeApi::default();
    let adapter = agent(
        FakeDiscovery {
            endpoints: vec![LanguageServerEndpoint {
                address: "10.1.2.3:62957".into(),
                token: "csrf-token".into(),
            }],
            ..FakeDiscovery::default()
        },
        api.clone(),
    );

    let error = adapter
        .resume(&session("conversation-1"), "继续")
        .await
        .unwrap_err();

    assert_eq!(error.code(), "antigravity_endpoint_rejected");
    assert!(api.probes().is_empty());
    assert!(api.sent().is_empty());
}

#[tokio::test]
async fn metadata_timeout_moves_on_to_the_next_endpoint() {
    let api = FakeApi {
        exists: HashMap::from([("conversation-1".to_owned(), "127.0.0.1:2".to_owned())]),
        slow_metadata_address: Some("127.0.0.1:1".to_owned()),
        ..FakeApi::default()
    };
    let adapter = AntigravityAgent::new_test().with_reply(
        reply(
            FakeDiscovery {
                endpoints: vec![endpoint(1), endpoint(2)],
                ..FakeDiscovery::default()
            },
            api.clone(),
        )
        .with_timeouts(Duration::from_millis(50), Duration::from_secs(5)),
    );

    adapter
        .resume(&session("conversation-1"), "继续")
        .await
        .unwrap();

    assert_eq!(api.sent().len(), 1);
    assert_eq!(api.sent()[0].0, "127.0.0.1:2");
}

#[tokio::test]
async fn send_timeout_is_unknown_and_not_retried() {
    let api = FakeApi {
        exists: HashMap::from([("conversation-1".to_owned(), "127.0.0.1:1".to_owned())]),
        send_delay: Some(Duration::from_secs(5)),
        ..FakeApi::default()
    };
    let adapter = AntigravityAgent::new_test().with_reply(
        reply(
            FakeDiscovery {
                endpoints: vec![endpoint(1)],
                ..FakeDiscovery::default()
            },
            api.clone(),
        )
        .with_timeouts(Duration::from_secs(1), Duration::from_millis(30)),
    );

    let error = adapter
        .resume(&session("conversation-1"), "继续")
        .await
        .unwrap_err();

    assert!(matches!(error, AgentError::Unknown(_)));
    assert_eq!(error.code(), "antigravity_send_unconfirmed");
    assert_eq!(api.sent().len(), 1, "Unknown 结果不得自动重试");
}

#[tokio::test]
async fn agent_api_failure_is_returned_without_retry() {
    let api = FakeApi {
        exists: HashMap::from([("conversation-1".to_owned(), "127.0.0.1:1".to_owned())]),
        send_error: Some(AgentError::Failed(safe_error(
            "antigravity_token_expired",
            "Antigravity 语言服务令牌已失效：请在桌面端重新打开该会话后重试",
        ))),
        ..FakeApi::default()
    };
    let adapter = agent(
        FakeDiscovery {
            endpoints: vec![endpoint(1)],
            ..FakeDiscovery::default()
        },
        api.clone(),
    );

    let error = adapter
        .resume(&session("conversation-1"), "继续")
        .await
        .unwrap_err();

    assert_eq!(error.code(), "antigravity_token_expired");
    assert!(error.to_string().contains("令牌已失效"), "{error}");
    assert_eq!(api.sent().len(), 1, "失败后不得重发");
}

#[tokio::test]
async fn blank_reply_text_is_rejected_before_discovery() {
    let api = FakeApi::default();
    let discovery = FakeDiscovery {
        endpoints: vec![endpoint(1)],
        ..FakeDiscovery::default()
    };
    let adapter = agent(discovery.clone(), api.clone());

    let error = adapter
        .resume(&session("conversation-1"), "   ")
        .await
        .unwrap_err();

    assert!(matches!(error, AgentError::InvalidInput));
    assert_eq!(discovery.calls(), 0, "空正文不得触发任何发现");
    assert!(api.sent().is_empty());
}
