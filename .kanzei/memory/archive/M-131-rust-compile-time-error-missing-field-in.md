---
id: M-131
scope: project
category: fact
title: Rust compile-time error: missing field in initializer (E0063)
description: 处理 Rust/Cargo 编译期错误时必读：字段缺失导致 E0xxx — 需检查结构体初始化参数完整性
status: deprecated
created: 2026-08-17
updated: 2026-08-18
source: memory-manager
subject: Rust compile-time field errors
---

适用场景：cargo test/compile 报 E0063 `missing field X in initializer of StructY`

操作步骤：
1) 读取完整 stderr → 确认缺失字段名（如 `background_notifications`）
2) 定位代码行号及结构体定义 → 检查构造函数参数列表
3) 修复：在 Init/Struct 定义中添加缺失字段，并赋予默认值或从上下文赋值

边界与例外：错误中包含路径信息说明是测试用例特定字段缺失（仍需记录复用模式），非 TDD 预期失败

Failure marker: [fp:bash|> error[E]: missing field in initializer of ] (来自 note 2)
Episode provenance source: episode_id=608 (已存在且真实)

(auto-deprecated: candidate 超出健康水位 24，按低价值优先清退；fingerprint=true，recurrence=0；原路径 C:\Users\kanzei\Documents\kanzei code\.kanzei\memory\M-131-rust-compile-time-error-missing-field-in.md)
