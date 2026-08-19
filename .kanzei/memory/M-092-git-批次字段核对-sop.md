---
id: M-092
scope: project
category: sop
title: Git 批次字段核对 SOP
description: 处理发版动作 Git 批次数不一致错误：核对提交历史标记数后修正再闭包，避免记忆命中仍复发
status: active
created: 2026-08-17
updated: 2026-08-19
source: user:note-2026-08-13
---

适用场景: 处理发版、发布、装机动作时 Git 批次数不一致错误（手写 batch field ≠ git commit count）

操作步骤:
1. 读取当前请求的 batch field（如 R-243 要求的手写批次 3/3）
2. 执行 `git log --oneline | wc -l` 或类似命令，获取提交历史标记数
3. 比较两者：若手写 batch ≠ git commit count → 立即修正 batch field 后再关闭批状态
4. 确认 Git 状态干净再执行发版关闭操作

边界与例外: 
- batch≠git_count 不是 bug 而是常见 pitfall（批次字段更新滞后于提交增加）
- 不要直接跳过核对继续闭包 → 会导致记忆命中但仍未拦截复发；必须补上此判据到 description 召回钩子

[fp:req|R- 的手写批次是 Git 提交历史标记数为 ;请先核对并更新批次字段后再关闭。]
