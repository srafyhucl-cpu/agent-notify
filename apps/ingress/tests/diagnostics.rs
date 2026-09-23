use std::fs;

use agentnotify_ingress::diagnostics::{DoctorReport, PipeStatus, SELF_VERSION, SpoolStatus};
use agentnotify_ingress::protocol::PROTOCOL_VERSION;
use agentnotify_ingress::spool::QUARANTINE_DIR;

#[test]
fn spool_status_counts_pending_and_quarantined_events() {
    let temp = tempfile::tempdir().expect("创建临时目录");
    let root = temp.path().join("spool");
    let quarantine = root.join(QUARANTINE_DIR);
    fs::create_dir_all(&quarantine).expect("创建隔离目录");
    fs::write(root.join("000-1.json"), "{}").expect("写入待补投事件");
    fs::write(root.join("000-2.json"), "{}").expect("写入待补投事件");
    fs::write(root.join("notes.txt"), "x").expect("写入非事件文件");
    fs::write(quarantine.join("000-3.json"), "{}").expect("写入隔离事件");

    let status = SpoolStatus::inspect(&root);
    assert!(status.exists);
    assert_eq!(status.pending, 2);
    assert_eq!(status.quarantined, 1);
    assert!(status.error.is_none());
}

#[test]
fn spool_status_counts_zero_quarantine_when_dir_is_absent() {
    let temp = tempfile::tempdir().expect("创建临时目录");
    let root = temp.path().join("spool");
    fs::create_dir_all(&root).expect("创建 spool 目录");
    fs::write(root.join("000-1.json"), "{}").expect("写入待补投事件");

    let status = SpoolStatus::inspect(&root);
    assert!(status.exists);
    assert_eq!(status.pending, 1);
    assert_eq!(status.quarantined, 0);
    assert!(status.error.is_none());
}

#[test]
fn spool_status_reports_missing_dir_without_error() {
    let temp = tempfile::tempdir().expect("创建临时目录");
    let status = SpoolStatus::inspect(&temp.path().join("absent"));
    assert!(!status.exists);
    assert_eq!(status.pending, 0);
    assert_eq!(status.quarantined, 0);
    assert!(status.error.is_none());
}

#[test]
fn doctor_report_is_ok_only_when_pipe_listens_and_spool_is_clean() {
    let temp = tempfile::tempdir().expect("创建临时目录");
    let listening = PipeStatus {
        name: Some(r"\\.\pipe\agentnotify-v1-test".to_owned()),
        listening: true,
        error: None,
    };
    assert!(DoctorReport::assemble(listening.clone(), SpoolStatus::inspect(temp.path())).ok);

    let offline = PipeStatus {
        name: Some(r"\\.\pipe\agentnotify-v1-test".to_owned()),
        listening: false,
        error: None,
    };
    assert!(!DoctorReport::assemble(offline, SpoolStatus::inspect(temp.path())).ok);

    let broken = PipeStatus {
        name: None,
        listening: false,
        error: Some("拿不到当前用户 SID".to_owned()),
    };
    assert!(!DoctorReport::assemble(broken, SpoolStatus::inspect(temp.path())).ok);

    fs::write(temp.path().join("000-1.json"), "{}").expect("写入待补投事件");
    assert!(!DoctorReport::assemble(listening, SpoolStatus::inspect(temp.path())).ok);
}

#[test]
fn doctor_report_json_carries_version_and_protocol() {
    let temp = tempfile::tempdir().expect("创建临时目录");
    let report = DoctorReport::assemble(
        PipeStatus {
            name: Some(r"\\.\pipe\agentnotify-v1-test".to_owned()),
            listening: true,
            error: None,
        },
        SpoolStatus::inspect(temp.path()),
    );

    let json = serde_json::to_string(&report).expect("诊断报告可序列化");
    assert!(json.contains(r#""ok":true"#));
    assert!(json.contains(&format!(r#""version":"{SELF_VERSION}""#)));
    assert!(json.contains(&format!(r#""protocol_version":{PROTOCOL_VERSION}"#)));
    assert!(report.summary().contains("正常"));

    let failing = DoctorReport::assemble(
        PipeStatus {
            name: Some(r"\\.\pipe\agentnotify-v1-test".to_owned()),
            listening: false,
            error: None,
        },
        SpoolStatus::inspect(temp.path()),
    );
    assert!(failing.summary().contains("异常"));
    assert!(failing.summary().contains("未在监听"));
}
