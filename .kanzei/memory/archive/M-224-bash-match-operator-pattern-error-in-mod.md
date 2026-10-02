---
id: M-224
scope: project
category: fact
title: bash match operator pattern error in mod.rs line 2049
description: 处理bash/match语法错误时必读：expected operator pattern错误是 Rust 语法问题，需修正match arm的pattern形式
status: deprecated
created: 2026-08-17
updated: 2026-08-18
source: memory-manager
subject: bash_match_syntax_error
---

bash本轮重复失败3次[fp:bash|test memory::store::tests::promote_write_evidence_failure_does_not_activate ...)。
exit code:1, stderr: expected one of `!`, `.`, `::`, `?`, `{`, or an operator, found `store`. 文件路径 \\?\C:\Users\kanzei\Documents\kanzei code\crates\kanzei-memory\src\memory\mod.rs:2049:9。涉及目标: cargo。复发档位:第3次(跨轮计数)。根因:rust代码中match语句使用`store`匹配但语法不正确,应为其他pattern类型。

(auto-deprecated: candidate 超出健康水位 24，按低价值优先清退；fingerprint=false，recurrence=0；原路径 C:\Users\kanzei\Documents\kanzei code\.kanzei\memory\M-224-bash-match-operator-pattern-error-in-mod.md)
