---
id: M-206
scope: project
category: fact
title: bash 编译 error[E]: mismatched types 的根因与验证步骤
description: 何时遇到 bash mismatched types 编译错误 — Cargo/Rust 类型不匹配的先决条件检查
status: candidate
created: 2026-08-17
updated: 2026-08-17
source: memory-manager
---

本次 bash 失败信号：exit code: 1 [stderr] Compiling tokio-rustls v0.26.4 → mismatched types [fp:bash|error[E]: mismatched types]
涉及目标：cargo

复发模式分析：第 1 次跨轮计数 → candidate 阶段，第 2 次带修复证据时晋升为 active。

可复用知识点（环境约束 + 工具契约）：
- Rust/Cargo 编译错误常由类型系统不一致触发
- tokio-rustls v0.26.4 版本与依赖项存在类型兼容问题
- 涉及 reqwest/kanzei-* crate 时需检查 feature flag 和 trait bounds
- bash 作为执行层无法修复，必须在 edit 阶段完成类型约束修正

验证步骤：
1. 查看完整 stderr，定位不匹配的 type 字段名
2. cargo check 先行静态校验（比编译更快）
3. 若为版本问题：upgrade/downgrade 至兼容版
4. 若为 trait 缺失：添加对应 crate 的 feature 启用标志

后续升级规则：2 次失败后建 candidate，第 3 次 + 修复成功证据时 memory_promote。
