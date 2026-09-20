use std::fs::File;

use agentnotify_runtime::{TelemetryConfig, init_telemetry};

const OVERSIZED_LOG_BYTES: u64 = 9 * 1024 * 1024;

/// 桌面宿主会在同一进程内多次启停运行时，日志初始化必须幂等且不会无限占用磁盘。
#[test]
fn runtime_log_initialization_is_idempotent_and_rotates_oversized_file() {
    let temp = tempfile::tempdir().unwrap();
    let log_path = temp.path().join("runtime.log");

    let oversized = File::create(&log_path).unwrap();
    oversized.set_len(OVERSIZED_LOG_BYTES).unwrap();
    drop(oversized);

    let _guard = init_telemetry(TelemetryConfig {
        log_path: log_path.clone(),
    })
    .expect("首次初始化运行时日志必须成功");

    let rotated = log_path.with_extension("log.1");
    assert_eq!(
        std::fs::metadata(&rotated).unwrap().len(),
        OVERSIZED_LOG_BYTES,
        "超过上限的历史日志必须被轮换"
    );
    assert_eq!(
        std::fs::metadata(&log_path).unwrap().len(),
        0,
        "轮换后必须新建空日志文件"
    );

    // 运行时重启会再次初始化，同一路径重复调用不能失败。
    let _second_guard = init_telemetry(TelemetryConfig {
        log_path: log_path.clone(),
    })
    .expect("重复初始化运行时日志必须成功");

    tracing::info!("运行时日志写入探针");
    let contents = std::fs::read_to_string(&log_path).unwrap();
    assert!(
        contents.contains("运行时日志写入探针"),
        "日志必须写入当前日志文件，实际内容：{contents}"
    );
}
