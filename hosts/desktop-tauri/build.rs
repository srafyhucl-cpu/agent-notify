fn main() {
    println!("cargo:rerun-if-changed=../../apps/desktop-ui/dist");
    tauri_build::build();
}
