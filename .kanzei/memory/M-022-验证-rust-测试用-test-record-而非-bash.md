---
id: M-022
scope: project
category: sop
title: 验证 Rust 测试必须用 test_record，禁止用 bash 跑 cargo test
description: 处理 Rust 测试验证，尤其 bash 返回 `exit code: 1` 且无有效输出时必读：不要重跑 bash 或把它当作代码诊断；改用 test_record 记录/验证结果，并先区分工具契约与代码问题。
status: active
created: 2026-08-09
updated: 2026-08-09
source: inbox:2026-08-09
---

处理 Rust 测试验证、尤其 bash 返回 `exit code: 1` 且 stderr 只有编译 warning/结果不明时，禁止用 bash 跑 cargo test；改用 test_record 记录并验证结果，先区分工具契约与代码问题，不要把 bash 失败当作测试诊断或通过重复执行寻找答案。

本轮错误原文：exit code: 1 (no output)。遇到该无输出失败时直接切换到 test_record，而不是继续 bash 重试。
[fp:bash|exit code:]
