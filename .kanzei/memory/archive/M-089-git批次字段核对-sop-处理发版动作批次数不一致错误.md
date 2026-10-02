---
id: M-089
scope: project
category: fact
title: Git批次字段核对 SOP — 处理发版动作批次数不一致错误
description: 处理发版 Git 批次数不一致/验收条款对账失败时必读：核对提交历史标记数后修正再闭包，逐条覆盖验收项并带证据锚（fp: req| 手写批次/验收条款未覆盖）
status: deprecated
created: 2026-08-17
updated: 2026-08-20
source: 2026-08-13 inbox consolidation [note 5]
superseded_by: M-258
---

R-243 手写批次是 Git 提交历史标记数；先核对并更新批次字段后再关闭。 [fp:req|R- 的手写批次是 Git 提交历史标记数为 ;请先核对并更新批次字段后再关闭。]+ R-243 验收条款对账未过:验收列出条款①②③④⑤⑥,其中①②③④⑤⑥在进展中未提及。关闭前每条验收必须在进展里逐条覆盖并带证据锚——test_record file:line/提交号;做不到的条款要显式写『验收降级:<条款号> 原文→实际+理由』,默
