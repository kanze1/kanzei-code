---
id: M-104
scope: project
category: fact
title: bash/cargo 编译 No field ... on type Result<...> 报错必读
description: 处理编译 no field on type 报错 — Result 类型缺少 status 字段导致方法调用失败,git.rs 处的 RUST 契约故障
status: deprecated
created: 2026-08-17
updated: 2026-08-18
source: memory-manager
---

处理 bash 重试中 "error[E0609]: no field `status` on type `Result<Output, std::io::Error>`" 失败时必读：这是 crates/kanzei-tools/src/git.rs:838 处的 RUST 契约故障。第 1 次跨轮计数失败触发。保留指纹：[fp:bash|> error[E]: no field on type Result<...>](refs: )

(auto-deprecated: candidate 超出健康水位 24，按低价值优先清退；fingerprint=true，recurrence=0；原路径 C:\Users\kanzei\Documents\kanzei code\.kanzei\memory\M-104-bash-cargo-编译-no-field-on-type-result-报错.md)
