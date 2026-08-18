---
id: M-159
scope: project
category: fact
title: req retry on unknown id after multiple failures
description: 处理 req反复失败时需先校验已知ID列表;当工具返回unknown id且存在已知记录时必读此条以判断是否为环境约束
status: candidate
created: 2026-08-17
updated: 2026-08-17
source: memory-manager
subject: req_unknown_id_retry
---

本次重试前：第2次失败（跨轮计数）, 错误原文显示unknown id R-216; existing已知ID列表:[R-286,R-283,R-284,R-285,R-287,R-235,R-101,R-242,R-243,R-245,R-248,R-249,R-264,R-281,R-288]。重试后成功, 确认此ID冲突为环境/工具契约类可复用知识,而非一次性噪声。
[fp:req|unknown id ; existing: R-, R-, R-, R-, R-, R-, R-, R-, R-, R-, R-, R-, R-, R-, R]
