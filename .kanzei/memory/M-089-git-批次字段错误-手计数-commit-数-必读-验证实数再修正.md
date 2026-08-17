---
id: M-089
scope: project
category: fact
title: git 批次字段错误(手计数≠commit 数)必读:验证实数再修正
description: 处理 req 批次数不匹配时必读：Git 标记 vs 手写 count 不一致 → 先核对再闭包
status: active
created: 2026-08-17
updated: 2026-08-17
source: 2026-08-13 inbox consolidation [note 5]
---

[fp:req|R- 的手写批次是 Git 提交历史标记数为 ;请先核对并更新批次字段后再关闭。]  
判断要点：发版时 R-175 error: "R-175 的手写批次是 6/6,但 Git 提交历史标记数为 5;请先核对并更新批次字段后再关闭" → action: verify actual commit count from git log, update batch field accordingly, then re-run git close。  
错误原文：R-175 的手写批次是 6/6,但 Git 提交历史标记数为 5;请先核对并更新批次字段后再关闭。
