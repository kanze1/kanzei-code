---
id: M-118
scope: project
category: sop
title: bash 编译错误重试策略：复发档位决定处理（NOOP/ADD/PROMOTE）
description: 处理 bash 编译错误重试策略必修：判断复发档位，决定 NOOP/ADD/PROMOTE，保留 [fp] 标记
status: candidate
created: 2026-08-17
updated: 2026-08-17
source: memory-manager
subject: bash compile failure retry rate
---

bash compile failures (case-insensitive ASCII, missing field, etc.)。  
**Rule**: 第 1 次失败 → NOOP（按"第2次建candidate"）；第 2 次失败且无修复证据 → memory_promote + episode evidence；第 3 次+ + 修复成功证据 → memory_add + promote。  

错误原文：manual case-insensitive ASCII comparison / missing field background_notifications
[fp:bash|> error[E]: manual case-insensitive ASCII comparison]
