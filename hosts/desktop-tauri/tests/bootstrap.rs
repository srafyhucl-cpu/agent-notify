#[test]
fn package_exposes_tauri_builder_without_starting_a_window() {
    let _builder = agentnotify_desktop::build_test_app();
    let config: serde_json::Value = serde_json::from_str(include_str!("../tauri.conf.json"))
        .expect("tauri.conf.json must be valid JSON");
    assert_eq!(config["productName"].as_str(), Some("AgentNotify"));
}
