use agentnotify_desktop::bridge::{
    BUSINESS_COMMAND_NAMES, CommandError, InstallUpdatePayload, InstallUpdateResultDto,
    RuntimeLifecycleStateDto, RuntimeSnapshotDto, RuntimeSummaryDto, SnapshotOverviewDto,
    UpdateStateDto,
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
            "retry_legacy_migration",
            "get_settings",
            "update_settings",
            "set_runtime_paused",
            "quit_app",
            "get_update_status",
            "install_update",
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
        migration: Default::default(),
    };

    let json = serde_json::to_value(snapshot).expect("快照必须可序列化");
    assert_eq!(json["runtime"]["appVersion"], "2.0.0-dev.0");
    assert_eq!(json["runtime"]["state"], "Running");
    assert_eq!(json["runtime"]["paused"], false);
}

#[test]
fn install_update_contract_keeps_empty_payload_and_five_state_result() {
    let payload = InstallUpdatePayload {};
    assert_eq!(
        serde_json::to_value(&payload).expect("空载荷必须可序列化"),
        serde_json::json!({})
    );
    // 序列化方向只要求字段齐全；反序列化必须容忍 UI 回传的空对象。
    assert_eq!(
        serde_json::from_value::<InstallUpdatePayload>(serde_json::json!({}))
            .expect("空对象必须可反序列化"),
        payload
    );

    let result = InstallUpdateResultDto {
        state: UpdateStateDto::ReadyToInstall,
        message: "更新包已校验，安装程序已启动（v2.1.0）。".into(),
        installed_version: Some("2.1.0".into()),
        signed: true,
        preview: false,
    };
    let json = serde_json::to_value(&result).expect("安装结果必须可序列化");
    assert_eq!(json["state"], "ReadyToInstall");
    assert_eq!(json["installedVersion"], "2.1.0");
    assert_eq!(json["signed"], true);
    assert_eq!(json["preview"], false);
    assert_eq!(json["message"], "更新包已校验，安装程序已启动（v2.1.0）。");

    // 五个状态由现有 UpdateStateDto 承载，安装结果不允许出现新状态。
    for state in [
        UpdateStateDto::UpToDate,
        UpdateStateDto::Available,
        UpdateStateDto::ReadyToInstall,
        UpdateStateDto::Unsupported,
        UpdateStateDto::Failed,
    ] {
        let json = serde_json::to_value(state).expect("状态必须可序列化");
        assert!(json.is_string(), "状态必须是稳定字符串: {json}");
    }
}
