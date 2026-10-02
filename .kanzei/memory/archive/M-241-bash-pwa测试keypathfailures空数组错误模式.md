---
id: M-241
scope: project
category: fact
title: bash PWA测试keyPathFailures空数组错误模式
description: 处理 bash keyPathFailures空数组错误：何时遇到PWA断言执行失败时必读
status: deprecated
created: 2026-08-17
updated: 2026-08-31
source: memory-manager
---

[fp:bash|"keyPathFailures": [],] 第1次复发记录。exit code:1配合"keyPathFailures":[],pwaUnpaired:[notifications(需配对...)]表明PWA组件检测通过但断言执行失败→检查notification配对状态或跳过断言。不要盲目重试bash。

(auto-deprecated: candidate 超过 14 个日历日未完成晋升，无满足条件的 recurrence/provenance；原路径 C:\Users\kanzei\Documents\kanzei code\.kanzei\memory\M-241-bash-pwa测试keypathfailures空数组错误模式.md)
