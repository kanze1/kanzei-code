---
id: M-116
scope: project
category: sop
title: 处理 cargo test 失败时必读：区分编译期/运行时错误（RUST_BACKTRACE）
description: 处理测试框架失败时必读：区分编译期/运行时错误，保留 [fp] 标记
status: deprecated
created: 2026-08-17
updated: 2026-08-20
source: memory-manager
subject: cargo test failures 判据
superseded_by: M-112
---

cargo test failures: test 框架失败而非编译错误。  
**Pitfall**: exit code=1 + "failures:" → 测试用例在 run，非编译期错误 → action: check RUST_BACKTRACE=1，分析具体 test case failure；若与已知 test failure 模式匹配（如 M-XXX）→ 针对性修复代码；否则属于一次性噪声 → NOOP。  

错误原文：failures: git::tests::finalize_rejects_fmt_before_tests
[fp:bash|failures:]
