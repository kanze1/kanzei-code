---
id: M-169
scope: project
category: fact
title: req 反复失败后重试成功模式
description: 当 req 调用失败且返回 unknown id+existing 已知 ID 列表时使用：这是工具契约失效的明确信号
status: deprecated
created: 2026-08-17
updated: 2026-08-17
source: memory-manager
refs: R-286 R-283 R-284 R-285 R-287
subject: R-216 req 失败模式
---

[fp:req|unknown id ; existing: R-286, R-283, R-284, R-285, R-287, R-235, R-101, R-242, R-243, R-245, R-248, R-249, R-264, R-281, R-288]
错误：unknown id `R-216`; existing: [上述 IDs]
场景：工具契约失效，req 调用时 ID 不存在于已知索引列表。重试成功证明问题可修复。已验证次数：2 次（失败→重试成功）。状态：candidate (未验证待 promotion)。

(stale: duplicate candidate of M-168; retain the first source-backed entry)
