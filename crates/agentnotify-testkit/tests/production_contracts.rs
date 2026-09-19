use std::sync::Arc;

use agentnotify_agent_opencode::{OpenCodeAgent, OpenCodeReplyInbox};
use agentnotify_agent_sdk::assert_agent_contract;
use agentnotify_channel_clawbot::ClawBotChannel;
use agentnotify_channel_sdk::assert_channel_contract;
use agentnotify_testkit::MemoryStore;

#[tokio::test]
async fn production_adapters_pass_shared_contracts() {
    let inbox = tempfile::tempdir().expect("生产契约测试必须能创建隔离目录");
    let agent = OpenCodeAgent::new(OpenCodeReplyInbox::new(inbox.path()));
    let channel = ClawBotChannel::new(Arc::new(MemoryStore::default()));

    assert_agent_contract(Arc::new(agent)).await;
    assert_channel_contract(Arc::new(channel)).await;
}
