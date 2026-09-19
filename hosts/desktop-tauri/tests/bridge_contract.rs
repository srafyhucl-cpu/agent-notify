use agentnotify_desktop::bridge::{
    BUSINESS_COMMAND_NAMES, CommandError, RuntimeLifecycleStateDto, RuntimeSnapshotDto,
    RuntimeSummaryDto, SnapshotOverviewDto,
};

#[test]
fn stable_command_names_are_exact_and_append_only() {
    assert_eq!(
        BUSINESS_COMMAND_NAMES,
        [
            "get_snapshot",
            "list_agents",
            "update_agent_config",
            "list_channel_accounts",
            "begin_channel_login",
            "submit_channel_login_code",
            "logout_channel_account",
            "enable_channel_account",
            "disable_channel_account",
            "send_test_notification",
            "list_notifications",
            "get_notification_detail",
            "retry_delivery",
            "get_diagnostics",
            "get_settings",
            "update_settings",
            "set_runtime_paused",
            "quit_app",
        ]
    );
}

#[test]
fn command_error_never_serializes_sensitive_fields() {
    let error = CommandError::new("channel_login_failed", "登录失败，请重新扫码")
        .with_retryable(true)
        .with_diagnostic_id("diagnostic-1");
    let json = serde_json::to_string(&error).expect("CommandError 必须可序列化");

    assert!(!json.contains("token"));
    assert!(!json.contains("authorization"));
    assert!(!json.contains("cookie"));
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&json).expect("错误 JSON 必须有效"),
        serde_json::json!({
            "code": "channel_login_failed",
            "message": "登录失败，请重新扫码",
            "retryable": true,
            "diagnosticId": "diagnostic-1",
        })
    );
}

#[test]
fn snapshot_dto_uses_camel_case_for_typescript_consumers() {
    let snapshot = RuntimeSnapshotDto {
        runtime: RuntimeSummaryDto {
            app_version: "2.0.0-dev.0".into(),
            platform: "windows".into(),
            state: RuntimeLifecycleStateDto::Running,
            paused: false,
        },
        overview: SnapshotOverviewDto::default(),
        components: Vec::new(),
        diagnostics: Vec::new(),
    };

    let json = serde_json::to_value(snapshot).expect("快照必须可序列化");
    assert_eq!(json["runtime"]["appVersion"], "2.0.0-dev.0");
    assert_eq!(json["runtime"]["state"], "Running");
    assert_eq!(json["runtime"]["paused"], false);
}
