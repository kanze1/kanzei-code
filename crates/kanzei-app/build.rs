fn main() {
    // 图标是 PE 资源输入。显式登记依赖，避免只替换 icon.ico 时 Cargo 复用
    // 旧 build-script 产物，出现“仓库图标已换、安装后的 exe 仍是旧图标”。
    println!("cargo:rerun-if-changed=icons/icon.ico");
    println!("cargo:rerun-if-changed=tauri.conf.json");
    tauri_build::build()
}
