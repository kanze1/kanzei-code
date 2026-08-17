---
id: M-128
scope: project
category: fact
title: bash 反复失败后重试成功(1 次)[fp:bash|manual case-insensitive ASCII comparison]
description: 处理 bash 编译后报 manual case-insensitive ASCII comparison 时必读:注意类型不匹配问题 — 区分 shell 与 rust 层面的错误来源
status: candidate
created: 2026-08-17
updated: 2026-08-17
source: memory-manager
---

[fp:bash|manual case-insensitive ASCII comparison] — bash 错误 exit code:1，提示"error: manual case-insensitive ASCII comparison"于 crates/kanzei-tools/src/docstore.rs:712。这是环境/工具契约类的可复用知识。判据：编译时类型不匹配或语法违规时，注意是 shell 还是 rust 层面的问题；若为 rust 错误码 E0xx → 按 R 错误分类处理，保留 fp 标记。
