---
id: M-022
scope: project
category: sop
title: 验证 Rust 测试必须用 test_record，禁止用 bash 跑 cargo test
description: 处理 Rust 测试验证、尤其 bash 返回 `exit code: 1` 或 stderr 只有编译 warning/测试结果不明时必读：不要再用 bash 执行 cargo test；改用 test_record 记录并验证结果，先区分工具契约与代码诊断，避免把 warning 门禁输出当成普通命令失败。
status: active
created: 2026-08-09
updated: 2026-08-09
source: inbox:2026-08-09
---

验证 Rust 测试必须使用 test_record，禁止用 bash 直接跑 cargo test。遇到 bash 的 `exit code: 1`，即使 stderr 只有 `warning: value assigned to `final_text` is never read`（crates\kanzei-core\src\runner.rs:1504:26），也不要重复 bash；改用 test_record，并分别记录/判断测试结果与编译 warning。

本条失败复发检测标记：[fp:bash|exit code:]
