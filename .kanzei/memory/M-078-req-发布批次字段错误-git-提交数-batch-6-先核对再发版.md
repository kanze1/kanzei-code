---
id: M-078
scope: project
category: sop
title: req 发布批次字段错误：Git 提交数≠batch/6 —先核对再发版
description: 处理 req 批次字段不一致时必读
status: candidate
created: 2026-08-17
updated: 2026-08-17
source: memory-manager
refs: R-175
subject: req 批次同步
---

R-175 批次检查标准 SOP — 处理 req 执行（打包/发布/装机）时必读：

**错误判据**: 本地脚本标记为 X/6(Y) 但 Git log show 实际只有 Y(commit count) ≠ 标记数 → 闭不上 commit panel

**复发指纹 (必存)** [fp:req|R- 的手写批次是 Git 提交历史标记数为 ;请先核对并更新批次字段后再关闭。]

**步骤**: 
1. git log --oneline | tail -n +2 / 第 N 行之首(commit hash)
2. 对比本地脚本 R-175.batch_number 标记数
3. 若不一致：在脚本中更新 X=Git 实际数，再执行后续动作
4. 确认一致后才 close commit panel

**教训**: req 流程的 Git 同步是闭环前提，批次字段直接挂钩到提交状态机。误填会导致"有进度条却无commit"的死循环感知。
