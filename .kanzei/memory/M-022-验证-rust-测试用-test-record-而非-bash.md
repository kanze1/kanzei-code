---
id: M-022
scope: project
category: sop
title: Rust/验证失败勿用 bash 反复跑测试，改用结构化验证
description: 处理 Rust 测试、verify.ps1 或 smoke probe 在 bash 返回 exit code 1、但输出包含具体业务断言失败时必读：不要重复 bash/cargo 重跑；先按断言定位实现问题，并用 test_record/结构化验证记录终态。
status: active
created: 2026-08-09
updated: 2026-08-10
source: inbox:2026-08-09
---

[fp:bash|exit code:]
既有规则：Rust 测试、verify.ps1 或 smoke probe 在 bash 返回 `exit code: 1` 时，不要把 bash 退出码直接当作代码失败，也不要反复重跑 cargo test；改用 `test_record`/结构化验证，并按具体错误区分工具失败或运行时数据问题。
本次复发的原文错误：`exit code: 1 ui-a11y-smoke.mjs ui-i18n-smoke.mjs ui-markdown-smoke.mjs ui-runtime-smoke.mjs --- UI 运行时冒烟失败 16 项：实质进展轮应计入推进轮次, 实得 0；写日记轮次第一次应记为无动作并追加推进指令；追加推进指令也应占推进轮次；刹车原因不对: 本轮完成`。这类输出应直接修正/验证 smoke 的业务断言，不因退出码重复执行同一命令。
