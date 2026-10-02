---
id: M-031
scope: project
category: sop
title: 关闭 req 前先收尾名下 running 测试记录
description: 处理 req 关闭被 running 测试记录阻塞时必读：先用 test_record 为对应测试 ID 写入 passed/failed/skipped 终态，再重试关闭；不再运行的测试必须 skipped 并说明原因。
status: deprecated
created: 2026-08-10
updated: 2026-08-12
source: memory-manager
---

[fp:req|r- 名下还有 条 running 测试记录没收尾,不能关闭:]
req 关闭有工具契约门禁：若报 `R-169 名下还有 1 条 running 测试记录没收尾,不能关闭: T-1786323304 cargo test --workspace (R-169 收口)`，先让该测试跑完，再用 `test_record` 带对应 ID 记录 `passed`/`failed`/`skipped` 终态；确实不跑了就记 `skipped` 并写明原因，然后再执行关闭。
