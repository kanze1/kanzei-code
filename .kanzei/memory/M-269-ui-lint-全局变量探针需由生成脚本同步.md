---
id: M-269
scope: project
category: fact
title: UI lint 全局变量探针需由生成脚本同步
description: 处理 UI 运行时冒烟提示 smoke probe marker 与源码不同步时必读：先运行 `node scripts/gen-ui-lint-glob` 重新生成标记文件，再重跑冒烟检查；不要把已通过的运行时错误数误判为失败根因。
status: active
created: 2026-08-20
updated: 2026-08-20
source: episode:920
---

UI 运行时冒烟可通过（25 个 ui/*.js、初始化序列 2318 次 invoke、10 个主视图切换、0 运行时错误），但 `ui-lint-globals.json` 可能与源码不同步并报告缺少 `lineStatusKey`、`lineStatusLabel` 等全局变量；应重跑 `node scripts/gen-ui-lint-glob` 后再验证。
