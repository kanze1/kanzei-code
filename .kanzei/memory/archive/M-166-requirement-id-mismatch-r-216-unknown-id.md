---
id: M-166
scope: project
category: fact
title: requirement id mismatch: R-216 unknown id
description: 处理 req id 映射失败问题时必读：判断是可复用环境契约还是一次性噪声
status: deprecated
created: 2026-08-17
updated: 2026-08-18
source: memory-manager
refs: R-165
---

unknown id R-216; existing IDs: R-286, R-283, R-284, R-285, R-287, R-235, R-101, R-242, R-243, R-245, R-248, R-249, R-264, R-281, R-288. 需显式在需求变更声明中指定 ID 映射，不可依赖编译期 auto-detect。

复发规则：第 1 次失败仅记录不立条目→第 2 次数值才建 candidate→第 3 次+修复成功证据用 memory_promote(episode_id=685)升 active。

[fp:req|unknown id ; existing: R-286, R-283, R-284, R-285, R-287, R-235, R-101, R-242, R-243, R-245, R-248, R-249, R-264, R-281, R-288]

(auto-deprecated: candidate 超出健康水位 24，按低价值优先清退；fingerprint=true，recurrence=0；原路径 C:\Users\kanzei\Documents\kanzei code\.kanzei\memory\M-166-requirement-id-mismatch-r-216-unknown-id.md)
