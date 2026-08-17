---
id: M-133
scope: project
category: fact
title: Rust type does not implement Copy error handling
description: 处理 Rust compile-time 错误：类型不实现 Copy trait — R-165 需用 episode_id=608
status: candidate
created: 2026-08-17
updated: 2026-08-17
source: memory-manager
---

适用场景：cargo test/compile 报 E0382 `type does not implement Copy`

操作步骤：
1) 读取完整 stderr → 确认类型不匹配错误
2) 检查结构体定义是否缺少实现 trait 的字段
3) 修复：在结构体中实现 Copy trait（添加所有必要字段使 struct 可 copy）

边界与例外：非一次性噪声；重复发生需建条复用模式

Failure marker: [fp:bash|> error[E]: manual case-insensitive ASCII comparison] (来自 note 1，但需注意该指纹实际在 stderr 中)
