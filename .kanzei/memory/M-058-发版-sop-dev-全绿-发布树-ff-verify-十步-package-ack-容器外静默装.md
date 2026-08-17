---
id: M-058
scope: project
category: sop
title: 执行发版动作时必读:标准步骤序列与三个已知坑位 (批次错配/前置未检查)
description: 处理 Git 批次数不一致错误必读：核对提交历史标记数再关闭批次
status: active
created: 2026-08-13
updated: 2026-08-17
source: 会话 2026-08-13 发版实操复盘(build-fe26bb7)
---

Git 发版时常见坑：1) 手写批次是手动写的数字/标记，可能与 Git 实际的提交数量不一致；2) error: R-175 的手写批次是 6/6,但 Git 提交历史标记数为 5;请先核对并更新批次字段后再关闭。行动：执行发版动作(打包/发布/装机)前，先 verify 实际 commit count 与手写 batch 字段是否一致，不一致则修正再闭包。[fp:req|R- 的手写批次是 Git 提交历史标记数为 ;请先核对并更新批次字段后再关闭。]
