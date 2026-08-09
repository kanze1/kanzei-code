---
id: M-022
scope: project
category: sop
title: 验证 Rust 测试必须用 test_record，禁止用 bash 跑 cargo test
description: 处理 Rust 测试验证、尤其 bash 返回 `exit code: 1` 且 stderr 只有编译 warning/结果不明时必读：不要把 bash 失败当作测试诊断或重跑；改用 test_record 记录并验证结果，先区分工具契约与代码问题。
status: active
created: 2026-08-09
updated: 2026-08-09
source: inbox:2026-08-09
---

验证 Rust 测试结果必须使用 test_record，禁止用 bash 执行 cargo test。bash 的 `exit code: 1` 可能只伴随编译 warning（例如 `warning: value assigned to final_text is never read`），不能据此判断测试失败或继续重复 bash；应改用 test_record 获取可判定的测试记录，并分别处理 warning 与真实测试结果。

[fp:bash|exit code:]
