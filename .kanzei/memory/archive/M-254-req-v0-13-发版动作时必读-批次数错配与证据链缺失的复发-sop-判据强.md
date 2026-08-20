---
id: M-254
scope: project
category: fact
title: req v0.13 发版动作时必读:批次数错配与证据链缺失的复发 SOP — 判据强化为「手写批次必等于 Git 提交」
description: 处理Req发版批次数错配:核对Git提交数再修正—当记忆命中仍复发时先read记忆内容再补全判据（[fp:req...]必须保留）
status: deprecated
created: 2026-08-18
updated: 2026-08-20
source: memory-manager
superseded_by: M-258
---

req v0.13 发版动作时必读:批次数错配与证据链缺失的复发SOP — 判据强化为「手写批次必等于Git提交」 — req反复失败判据失效后的强化SOP

**适用场景**: R-243发版时Git提交历史标记数(human-readable)与手写批次字段不一致，或验收条款对账未过。

**操作步骤**:  
1. 读取当前请求的batch field（如[R-243]标注的手写批次）。
2. 执行`git log --oneline`确认Git提交历史标记数。
3. 核对两者是否一致——不一致即报错「R-243手写批次是X/3，但Git提交历史标记数为Y」。
4. 在进展中修正batch field使其等于Git计数后再关闭发版动作。

**边界与例外**:  
- 如果用edit/memory_writer写入后仍错配→检查是否触发whole-file-rewrite guard拦截。
- 若验收条款①~⑥未逐条覆盖并有证据锚→按M-061处理做「验收降级」声明或用户执行标注。

**判据强化**: [fp:req|R- 的手写批次是 Git 提交历史标记数为 ;请先核对并更新批次字段后再关闭。]
