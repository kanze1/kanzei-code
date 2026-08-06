# Defects

## D-026 R-014 多模态运行入口未导出新 runner API 导致桌面端编译失败 [fixed] (medium)
- 修复: 补充 pub use 导出并回归测试
- 原因: kanzei-app 引用了 run_once_with_parts，但 kanzei-core lib.rs 未公开导出
- 复现: cargo test -p kanzei-core -p kanzei-app
- 优先级: P1

## D-027 最后一步收走工具但未告知模型:codex 把工具调用当纯文本狂喷 JSON 并反复自我纠正,收尾轮完全失效 [fixed] (medium)
