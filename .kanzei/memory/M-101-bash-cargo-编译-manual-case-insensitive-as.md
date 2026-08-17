---
id: M-101
scope: project
category: fact
title: bash/cargo 编译 manual case-insensitive ASCII comparison 失败必读
description: 处理 bash/cargo 编译中 case-insensitive ASCII comparison 失败时的重试策略 — 手动小写对比错误时必读：这是环境/工具契约导致的可复用问题,非一次性噪声
status: candidate
created: 2026-08-17
updated: 2026-08-17
source: memory-manager
---

处理 bash 重试中 "error: manual case-insensitive ASCII comparison" 失败时必读：这是 crates/kanzei-tools/src/docstore.rs:712 处的环境工具契约导致的可复用故障模式,非 TDD 预期噪声。第 1 次跨轮计数失败的重复验证后触发。保留指纹：[fp:bash|> error: manual case-insensitive ASCII comparison]。晋升规则：第 3 次 + 修复成功证据时 memory_promote 至 active。(refs: )
