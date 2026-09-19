use std::{panic::AssertUnwindSafe, sync::Arc};

use agentnotify_domain::{AgentId, AgentSessionId, RequestId};

use crate::{AgentAdapter, AgentError, AgentEventEnvelope};

/// 在无网络条件下验证所有 Agent 适配器共有的稳定约束。
pub async fn assert_agent_contract(adapter: Arc<dyn AgentAdapter>) {
    let descriptor = adapter.descriptor();
    assert!(
        !descriptor.display_name.trim().is_empty(),
        "Agent descriptor display_name 不能为空"
    );
    assert_eq!(
        descriptor,
        adapter.descriptor(),
        "Agent descriptor 必须保持稳定"
    );

    let session_id = AgentSessionId::new("contract-session").unwrap();
    let capabilities = adapter.capabilities();
    if capabilities.resume {
        let result = adapter.resume(&session_id, "   ").await;
        assert!(
            matches!(result, Err(AgentError::InvalidInput)),
            "支持 resume 的 Agent 必须拒绝空白正文"
        );
    } else {
        let result = adapter.resume(&session_id, "继续处理").await;
        assert!(
            matches!(result, Err(AgentError::UnsupportedCapability)),
            "不支持 resume 的 Agent 必须返回 UnsupportedCapability"
        );
    }

    let unknown_event = AgentEventEnvelope {
        request_id: RequestId::new("contract-request").unwrap(),
        agent_id: AgentId::new("contract-agent").unwrap(),
        payload: serde_json::json!({"eventType": "agentnotify.unknown"}),
    };
    let result = std::panic::catch_unwind(AssertUnwindSafe(|| adapter.parse_event(unknown_event)));
    match result {
        Ok(Err(AgentError::InvalidEvent)) => {}
        Ok(Err(other)) => panic!("未知事件必须返回 InvalidEvent，实际为 {other:?}"),
        Ok(Ok(_)) => panic!("未知事件不能产生标准化通知"),
        Err(_) => panic!("Agent parse_event 不能 panic"),
    }
}
