use std::sync::Arc;

use agentnotify_agent_opencode::{OpenCodeAgent, OpenCodeReplyInbox};
use agentnotify_agent_sdk::{AgentAdapter, assert_agent_contract};

#[tokio::test]
async fn adapter_passes_agent_contract() {
    let agent = OpenCodeAgent::new(OpenCodeReplyInbox::new(std::env::temp_dir().join(format!(
        "agentnotify-opencode-contract-{}",
        uuid::Uuid::new_v4()
    ))));
    assert_agent_contract(Arc::new(agent)).await;
}

#[test]
fn descriptor_and_capabilities_are_stable() {
    let adapter = OpenCodeAgent::new(OpenCodeReplyInbox::new(std::env::temp_dir()));
    assert_eq!(adapter.descriptor().id.as_str(), "opencode");
    assert!(adapter.capabilities().notify);
    assert!(adapter.capabilities().resume);
    assert!(adapter.capabilities().session_title);
    assert!(adapter.capabilities().hook_installer);
    assert!(!adapter.capabilities().reply_window);
}
