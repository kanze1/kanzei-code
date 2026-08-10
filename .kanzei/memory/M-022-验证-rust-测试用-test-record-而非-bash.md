---
id: M-022
scope: project
category: sop
title: Rust/验证失败勿用 bash 反复跑测试，改用结构化验证
description: 处理 Rust 测试、verify.ps1 或 smoke probe 在 bash 返回 exit code 1 时必读：不要把 bash 退出码直接当代码失败或重复重跑 cargo test；改用 test_record/结构化验证，并按具体错误区分工具或运行时数据问题。
status: active
created: 2026-08-09
updated: 2026-08-10
source: inbox:2026-08-09
---

验证 Rust 测试时必须用 test_record，禁止用 bash 跑 cargo test；bash 的 exit code 1 不足以判定代码失败，应切换到结构化验证工具并据结果定位。\n\n本轮错误原文：exit code: 1 令牌 smoke probe marker；UI 运行时冒烟失败： - reportPersistentError: Failed to load architecture index:TypeError: Cannot read properties of null (reading 'design_docs')\n[fp:bash|exit code:]
