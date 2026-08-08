# Defects

## D-152 Windows 安装包在 64 位 Windows 上提示不兼容 [open] (high)
- 复现: 运行发布产物 kanzei-setup.exe，Windows 提示“由于与64位版本的Windows不兼容，此程序或功能无法运行”，路径位于 AppData\Local\Temp\kz-helper-*\kanzei-setup.exe。
- 标签: 发布
- 根因: 待确认：当前构建脚本直接使用本机 cargo tauri build，产物名为 kanzei_0.1.0_x64-setup.exe；需核验安装器 PE 架构及更新器下载/替换链路。
- 进展: 已确认 package.ps1 未显式指定 target，Tauri NSIS 产物为 x64 命名；尚未修改。
- 验收: 在受影响的 64 位 Windows 环境运行正式安装包成功启动安装，并验证应用内下载的安装器同样可执行；保留自动化构建/架构检查。
- 优先级: P0
