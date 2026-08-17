---
id: M-078
scope: project
category: sop
title: req 发布前版本核对:Git提交数≠batch导致失败 — 先核对再发版
description: 处理 req v0.13 批次字段错误时必读：核对Git提交数再发版，否则发布失败
status: active
created: 2026-08-17
updated: 2026-08-17
source: memory-manager
refs: R-175
subject: req 批次同步
---

处理 req 发布批次字段错误：Git 提交数≠batch/6。标准步骤：(1)核对本地脚本 [R-175].[batch]_[number]标记，比较Git历史提交计数与hand-written batch值；(2)如不一致先修正批次字段再发版。(3)[fp:req|R- 的手写批次是 Git 提交历史标记数为 ;请先核对并更新批次字段后再关闭。]关键判据：批次数差≠测试失败，是发版流程必查项。
