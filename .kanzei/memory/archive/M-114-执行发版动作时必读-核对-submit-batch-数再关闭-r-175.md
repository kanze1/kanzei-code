---
id: M-114
scope: project
category: sop
title: 执行发版动作时必读：核对 submit batch 数再关闭（R-175）
description: 处理 batch count mismatch 错误后必读：核对提交数修正 batch 字段，保留 [fp] 标记
status: deprecated
created: 2026-08-17
updated: 2026-08-18
source: memory-manager
subject: git batch count mismatch 判据
---

R-175 手写批次数与 Git commit 数不一致的纠正流程：  
**Pitfall**: error "R-175 的手写批次是 X/6,但 Git 提交历史标记数为 Y" → 说明手工填写的 batch count 与实际提交数不符 → action: verify actual commit count，修正 batch field 后再次 run git close。  

错误原文：R-175 的手写批次是 6/6,但 Git 提交历史标记数为 5;请先核对并更新批次字段后再关闭。
[fp:req|R- 的手写批次是 Git 提交历史标记数为 ;请先核对并更新批次字段后再关闭.]

(auto-deprecated: candidate 超出健康水位 24，按低价值优先清退；fingerprint=true，recurrence=0；原路径 C:\Users\kanzei\Documents\kanzei code\.kanzei\memory\M-114-执行发版动作时必读-核对-submit-batch-数再关闭-r-175.md)
