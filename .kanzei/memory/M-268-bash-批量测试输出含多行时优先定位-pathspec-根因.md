---
id: M-268
scope: project
category: fact
title: bash 批量测试输出含多行时优先定位 pathspec 根因
description: 处理 bash/test runner 批量测试输出混杂多行、看到 exit code: 1 与大量 running/ok 行时必读：先截取并核对完整 stderr/pathspec 根因行及测试结束标记，再决定修复；不要把首个 exit code、进度中的 ok 或截断片段当作 pathspec 根因，也不要在未定位根因前重跑。
status: active
created: 2026-08-20
updated: 2026-08-21
source: user
---

判据：批量测试输出含多行时，`exit code: 1` 以及 `running 253 tests ... ok` 等进度/通过行本身不能说明失败根因；先读取完整输出，定位真实 stderr/pathspec/路径错误行，并核对最终测试结果，再按根因修复或改用结构化 test_record 后重跑。复发证据原文：`exit code: 1  running 253 tests test experience_events::tests::unknown_legacy_event_is_diagnostic ... ok test history::tests::keeps_error_and_cancel_results_and_drops_unknown_results ... ok test history::tests::duplicate_ids_pair_one_to_one`。 [fp:bash|test runner::metrics::tests::failure_kind_多行bash批次_优先取pathspec根因行 ... ok] [fp:bash|行动: 处理 runner 批量测试失败且输出混杂多行、尤其出现 pathspec 或路径错误时必读:先从 exit code 当成根因，也不要盲目重试。] [fp:bash|行动: 处理 runner 批量测试失败且输出混杂多行、尤其出现 pathspec、路径错误或具体 stderr exit code test_record 获]
