---
id: M-218
scope: project
category: fact
title: bash/rustc 命令重复失败累积知识 (M-160::store match)
description: 处理 bash/rustc 命令重复失败累积知识：记录 fp:bash|... - 第3次+修复成功则 promote;若复发再建条目。
status: deprecated
created: 2026-08-17
updated: 2026-08-17
source: run:2026-08-17
---

[fp:bash|test memory::store::tests::promote_write_evidence_failure_does_not_activate ... ]

根因：memory::store match arm 在 bash/rustc 环境下 token 解析失败（路径 shell 参数差异导致）。第1-2次作为候选记录;第3+次且修复成功则 promote。
适用场景 + 操作步骤:
- cargo build/dev 遇到 exit code:1，error: expected one of ! . :: ? {, 或 test memory::store::tests promotion issue。
- 步骤①：检查源码行号/路径是否存在转义或 shell 参数异常。
- 步骤②：git diff 确认未提交改动（rustc 建议）。

边界与例外：非 bash/rustc环境；仅针对 M-160 mod.rs 的 store match arm 问题。此条目满足“第3次 + 成功证据”条件，将用 episode_id=752 promote to active。

(stale: 被新指纹更精确覆盖：fp:bash|test memory::store::tests::promote_write_evidence_failure_does_not_activate... 与M-218原指纹bsh|test [memory]::store::tests::promote_writetest不完全匹配，需添加新证据增强判据)
