# Test Runs Archive


## T-1786220501 D-200 设置页静态文案 i18n 登记 + 前端冒烟四连 [passed]
- 命令: node scripts/ui-i18n-smoke.mjs; node scripts/ui-runtime-smoke.mjs; node scripts/ui-a11y-smoke.mjs; node scripts/ui-markdown-smoke.mjs
- 摘要: D-200 修复后四条前端冒烟全绿:i18n(33 资源 key/70 HTML 文案/1 动态契约)、runtime(123 invoke 初始化+6 视图切换 0 错误)、a11y、markdown。