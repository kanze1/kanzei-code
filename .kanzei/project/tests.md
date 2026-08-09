# Test Runs


## T-1786317697 五项交付验证(merge_ff/活动流降噪/实时输出/侧栏开合/发版步进) [passed]
- 命令: cargo fmt --check && cargo test --workspace;node scripts/ui-sources.mjs + ui-runtime-smoke.mjs + ui-a11y-smoke.mjs + ui-i18n-smoke.mjs;package.ps1 无参步进冒烟
- 摘要: workspace 13 个测试目标全绿(含新增 git merge_ff 3 测、harness progress 1 测、tool_exec 并发回归);UI 四冒烟通过,新增小工具降噪/bash 实时流/rail 侧栏开合断言;package.ps1 打出 [1/6] 步进后按预期被 Ack 门禁拦截。
- 收尾: 1786317697
