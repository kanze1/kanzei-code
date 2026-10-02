---
id: M-192
scope: project
category: fact
title: git fatal: 单 Revision 缺失致同步失败
description: git fatal: Needed a single revision 根因：单 Revision 状态缺失导致同步失败
status: deprecated
created: 2026-08-17
updated: 2026-08-19
source: memory-manager
---

[fp:bash|fatal: Needed a single revision]
根因：git命令执行时无单 Revision状态，表明分支/工作区存在冲突或未完成的更改。此错误通常发生在多次重试后仍无法自动resolve时，需先手动解决冲突或重置工作区再重新执行同步命令。属于工具契约层面的固定场景，重复出现率较高，不应视为一次性噪声。

(auto-deprecated: candidate 超出健康水位 24，按低价值优先清退；fingerprint=true，recurrence=0；原路径 \\?\C:\Users\kanzei\Documents\kanzei code\.kanzei\memory\M-192-git-fatal-单-revision-缺失致同步失败.md)
