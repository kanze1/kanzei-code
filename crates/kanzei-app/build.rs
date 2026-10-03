fn main() {
    // 同一路径升级 exe 后，后台进程可能还活着。握手标识必须编入二进制，
    // 不能读取已被替换的磁盘文件来推断旧进程版本。
    // Release identifiers, embedded UI and shared-crate changes must also
    // invalidate the handshake. Cargo may otherwise relink a new executable
    // while reusing this build script's old timestamp.
    println!("cargo:rerun-if-env-changed=KANZEI_BUILD_INFO");
    println!("cargo:rerun-if-changed=ui");
    println!("cargo:rerun-if-changed=../../Cargo.toml");
    println!("cargo:rerun-if-changed=../../Cargo.lock");
    for entry in std::fs::read_dir("..").expect("workspace crates") {
        let root = entry.expect("workspace crate").path();
        if root.join("Cargo.toml").is_file() {
            println!(
                "cargo:rerun-if-changed={}",
                root.join("Cargo.toml").display()
            );
            println!("cargo:rerun-if-changed={}", root.join("src").display());
        }
    }
    let revision = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("build clock")
        .as_nanos();
    println!("cargo:rustc-env=KANZEI_RUNTIME_BUILD={revision}");
    // 图标是 PE 资源输入。显式登记依赖，避免只替换 icon.ico 时 Cargo 复用
    // 旧 build-script 产物，出现“仓库图标已换、安装后的 exe 仍是旧图标”。
    println!("cargo:rerun-if-changed=icons/icon.ico");
    println!("cargo:rerun-if-changed=tauri.conf.json");
    tauri_build::build()
}
