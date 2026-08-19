---
id: M-082
scope: project
category: sop
title: 执行发版动作时必读:核对 submit batch 数再关闭
description: 处理 batch count mismatch 错误后的纠正必读
status: deprecated
created: 2026-08-17
updated: 2026-08-18
source: 2026-08-13 inbox consolidation
---

R-175 的手写批次是 6/6,但 Git 提交历史标记数为 5;请先核对并更新批次字段后再关闭。错误原文常带 R-175 标记，说明手写 count 与真实 commit 数量不一致。行动：发版时标准步骤序列 1)verify actual commit count 2)修正 batch field 3)再次 run git close。

(auto-deprecated: candidate 超出健康水位 24，按低价值优先清退；fingerprint=false，recurrence=0；原路径 C:\Users\kanzei\Documents\kanzei code\.kanzei\memory\M-082-执行发版动作时必读-核对-submit-batch-数再关闭.md)
