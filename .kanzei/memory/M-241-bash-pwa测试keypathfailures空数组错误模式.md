---
id: M-241
scope: project
category: fact
title: bash PWA测试keyPathFailures空数组错误模式
description: 处理 bash keyPathFailures空数组错误：何时遇到PWA断言执行失败时必读
status: candidate
created: 2026-08-17
updated: 2026-08-17
source: memory-manager
---

[fp:bash|"keyPathFailures": [],] 第1次复发记录。exit code:1配合"keyPathFailures":[],pwaUnpaired:[notifications(需配对...)]表明PWA组件检测通过但断言执行失败→检查notification配对状态或跳过断言。不要盲目重试bash。
