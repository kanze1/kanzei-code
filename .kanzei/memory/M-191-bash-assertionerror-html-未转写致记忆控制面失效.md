---
id: M-191
scope: project
category: fact
title: bash AssertionError: HTML 未转写致记忆控制面失效
description: bash AssertionError根因：HTML静态文案未进入资源表导致记忆控制面失效
status: candidate
created: 2026-08-17
updated: 2026-08-17
source: memory-manager
---

[fp:bash|AssertionError [ERR_ASSERTION]: HTML 静态文案未进入资源表: 记忆控制面]
根因：UI运行时测试前需先验证HTML内容是否已正确转写入库（记忆控制面）。当bash命令执行ui-lint-globals.json同步720个标识符时，若返回AssertionError，表明存在HTML静态文案未进入资源表的情况。此问题具有较高复发率，是工具契约层面的已知模式，需在步骤3(bash)前提前验证HTML转写完整性，否则必然触发该断言失败。
