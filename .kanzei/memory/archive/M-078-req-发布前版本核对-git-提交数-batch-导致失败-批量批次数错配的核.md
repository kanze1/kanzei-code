---
id: M-078
scope: project
category: sop
title: req 发布前版本核对:Git 提交数≠batch 导致失败 — 批量批次数错配的核心判据与操作
description: 处理 req 批次字段错误时必读：比对 Git 提交数≠batch→修正后重试，否则复发——判据强化为「手写批次与 Git 标记必须一致」
status: deprecated
created: 2026-08-17
updated: 2026-08-18
source: memory-manager
refs: R-175
subject: req 批次同步
---

R-243 的手写批次是 Git 提交历史标记数；请先核对并更新批次字段后再关闭。闭锁「Git 批次数≠手写批次」错误：修正后重试，否则复发。[fp:req|R- 的手写批次是 Git 提交历史标记数为 ;请先核对并更新批次字段后再关闭。]

(stale: 被 M-235 替代：权限拒绝模式已覆盖 bash guard whole-file rewrite 的判据，无需重复。)
