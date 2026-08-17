---
id: M-103
scope: project
category: fact
title: bash/cargo 编译 Error[E0382]: the type does not implement Cop y 必读
description: 处理 Arc 类型不可 Copy 的编译报错 — Arc 默认不支持复制操作,编译器会拒绝一元/复制使用场景
status: candidate
created: 2026-08-17
updated: 2026-08-17
source: memory-manager
---

处理 bash 重试中 "error[E0382]: the type `Arc` does not implement `Copy`" 失败时必读：这是 crates/kanzei/tests/background_subagent_dispatch.rs:31 处的 Rust 契约故障,Arc 默认不可复制。第 1 次跨轮计数失败触发。保留指纹：[fp:bash|> error[E]: the type does not implement Copy]。解决方案：避免对 Arc 使用 copy/move 上下文或改用 Rc/引用传递。(refs: )
