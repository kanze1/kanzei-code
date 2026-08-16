# Test Runs

## T-1786922726035 R-285 金色神经流前端回归 [passed]
- 命令: UI 全部 JS node --check;npm run lint;node scripts/ui-runtime-smoke.mjs;node scripts/ui-a11y-smoke.mjs;node scripts/ui-i18n-smoke.mjs;node scripts/ui-markdown-smoke.mjs;node scripts/parallel-lines-regression.mjs;node scripts/ui-connectivity.mjs
- 摘要: 24 个 UI 脚本按 index.html 顺序初始化通过(2114 次 invoke,10 个主视图,0 运行时错误);R-285 画布/API/事件接线断言通过;ESLint、1226 i18n key、无障碍、Markdown、并行线路和 10/10 导航连通性全绿。
- 证据等级: E2(模拟 Tauri 运行时+跨脚本事件契约;不代表真实 WebView2 帧率)
- 关联: R-285

## T-1786922726036 R-285 Chromium Canvas 视觉验收 [passed]
- 命令: playwright-cli 打开 output/playwright/neural-flow-preview.html(生产 style.css+22-neural-flow.js),1440x1000 与 800x720 截图,运行态循环触发 memory_recall_injected/memory_candidate_promoted
- 摘要: 真实 Chromium Canvas 下呼吸/流动/结晶可见;主对话神经场集中在右侧外围且未压正文;记忆页强度更高;800px 构图仍成立。截图:output/playwright/neural-flow-active.png、neural-flow-800.png。唯一 console error 为预览页 favicon.ico 404,与产品代码无关。
- 证据等级: E2(真实浏览器渲染生产 JS/CSS;未启动 Tauri、未测 WebView2 长会话性能)
- 关联: R-285
