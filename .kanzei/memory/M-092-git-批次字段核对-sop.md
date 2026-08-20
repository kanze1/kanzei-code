---
id: M-092
scope: project
category: sop
title: Git 批次字段核对 SOP
description: 处理发版动作 Git 批次数不一致/验收条款对账失败时必读:先核对 git 提交历史标记数再更新批次字段后关闭,不可用手写批次直接 close;验收条款逐条在进展中覆盖并带证据锚(T- 测试记录/file:line/提交号),做不到须显式写『验收降级』,沉默跳过即拒。复发锚点:R-243 手写批次 3/3 vs Git 提交历史标记数 4(第 2+ 次同类复发);R-243 验收六条款在进展中全部未提及 → 对账失败。
status: active
created: 2026-08-17
updated: 2026-08-20
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
