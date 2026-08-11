---
id: M-037
scope: project
category: fact
title: R-178在dev交付后勿在其上 repeat实现 -检查git log--所有看独立表列方案是否冲突再动
description: 检查目标 R/SO/REQ 条目是否已按另一交付线关闭 —动工前 git log --all核对互斥关系避免重复实现
status: candidate
created: 2026-08-11
updated: 2026-08-11
source: memory-manager
---

R-178 已在 dev 完整交付并关闭(批1 d575549/2 c597d0a/3 540f178采用manual_models列方案/4 ba616f7 D7域选择器收口 a9a2ecc)。但 thread-line-20260811062027工作树留著独立implementation(手动表+add/command/schemav12),与dev权威互斥。已丢弃并 merge_ff 对齐 dev。教训：动工前先 git log --all 查是否已被其他线交付;R-178 的 manual_models是 processes 表的列(JSON数组)非独立表。
