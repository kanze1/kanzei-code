#[path = "build/ipc.rs"]
mod ipc;

fn main() {
    ipc::generate();
    println!("cargo:rerun-if-env-changed=KANZEI_BUILD_INFO");
    println!("cargo:rerun-if-changed=dist-web");
    println!("cargo:rerun-if-changed=icons/icon.ico");
    println!("cargo:rerun-if-changed=tauri.conf.json");
    tauri_build::build()
}
