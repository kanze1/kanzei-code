---
id: M-022
scope: project
category: sop
title: 验证 Rust 测试必须用 test_record，禁止用 bash 跑 cargo test
description: 处理 Rust 测试验证，尤其 bash 返回 `exit code: 1` 且无有效输出时必读：停止重跑 bash，把它视为工具契约问题；改用 test_record 记录/验证，并据其结果区分工具问题与代码问题。
status: active
created: 2026-08-09
updated: 2026-08-09
source: inbox:2026-08-09
---

Rust 测试验证必须通过 `test_record`，禁止用 bash 直接跑 `cargo test`。当 bash 只返回 `exit code: 1 (no output)` 时，不要重试或据此诊断代码；改用 `test_record`，并先确认请求/字段契约。

[fp:bash|exit code:]
