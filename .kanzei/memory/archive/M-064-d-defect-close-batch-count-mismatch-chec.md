---
id: M-064
scope: project
category: sop
title: D-defect close batch-count-mismatch check before ack
description: 处理 release defect close fail/批次计数不一致时需必读：验证手写批次数与 commit history 是否对齐前执行关闭操作
status: deprecated
created: 2026-08-13
updated: 2026-08-17
source: memory-manager
---

[RFP:defect] 关闭缺陷 D-IDX-DYZZ 前必须核对：手写作业批次编号与 Git commit log timestamp-count-matches。错误案例：D-331，手写批次数为 X/XX 但提交历史显示 XX 次；根本原因是发版窗口计算未更新或与开发进度脱节。

(stale: defect 关闭前批次计数校验已经成为 tracker close 的代码硬门禁；该 candidate 历史召回与采纳均为 0，继续保留会形成双源。)
