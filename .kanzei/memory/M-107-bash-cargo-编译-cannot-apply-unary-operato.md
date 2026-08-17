---
id: M-107
scope: project
category: fact
title: bash/cargo 编译 cannot apply unary operator to type impl Future<...> 报错必读
description: 处理一元运算符应用到 Future 类型的编译报错 — !gone 对 impl futures::Future<bool>无效
status: candidate
created: 2026-08-17
updated: 2026-08-17
source: memory-manager
---

处理 bash 重试中 "error[E0600]: cannot apply unary operator `!` to type `impl futures::Future<Output = bool>`" 失败时必读：这是 crates/kanzei-tools/src/background.rs:1381 处的错误用法。第 1 次跨轮计数失败触发。保留指纹：[fp:bash|> error[E]: cannot apply unary operator to type impl Future<...>](refs: )
