---
id: M-098
scope: project
category: sop
title: edit 报 old_string not found 时须分两案处理：构造精确 match vs 旧文丢失
description: 处理 old_string not found 失败时必读：区分构造错误（Case1）vs旧文丢失（Case2），补全判据并保留 [fp] 标记
status: candidate
created: 2026-08-17
updated: 2026-08-17
source: memory-manager
subject: edit old_string not found 判据
---

old_string not found 错误的两案处理：  
**Case 1**: "old_string not found in Y — Closest line: '...'" → old_string 构造错误（含空白/缩进不对） → action: read 重读源文件，逐字构造 old_string（含空白），再试。  
**Case 2**: 旧文被顶掉且没有报错 old_string not found → 但 new_string 多了行、old 没保住 → 已用 M-089 的三步判据处理。  

错误原文：old_string not found in C:\...\drive.rs — it must match exactly, including whitespace. Closest line in file: '                            let results = rt.background_results.c
[fp:edit|old_string not found in — it must match exactly, including whitespace.]
