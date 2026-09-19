use std::path::PathBuf;

fn main() {
    let output =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../apps/desktop-ui/src/bridge/types.ts");
    agentnotify_desktop::bridge::export_typescript_bindings(&output)
        .expect("导出 HostBridge TypeScript 类型失败");
}
