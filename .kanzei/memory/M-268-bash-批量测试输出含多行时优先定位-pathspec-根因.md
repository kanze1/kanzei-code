---
id: M-268
scope: project
category: fact
title: bash 批量测试输出含多行时优先定位 pathspec 根因
description: 处理 bash/test runner 批量测试失败且输出混杂多行时必读:先从 stderr/输出中定位最具体的 pathspec 根因行，再修正测试目标或参数，不要把整段批量输出误判为单一故障。
status: active
created: 2026-08-20
updated: 2026-08-20
source: user
---

环境/工具契约：多行 bash 测试批次输出应优先取 pathspec 根因行进行诊断；本次记录的输出含 `exit code: 0` 与多项测试 `ok`，不能仅凭混杂的批次文本判定失败。原错误/证据：exit code: 0  running 38 tests ... ok。复发指纹：[fp:bash|test runner::metrics::tests::failure_kind_多行bash批次_优先取pathspec根因行 ... ok]
