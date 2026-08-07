---
id: M-010
scope: project
category: sop
title: edit 报 old/new 相同是 no-op 拒绝而非失败
description: 处理 edit 报 "old_string and new_string are identical — nothing to do" 时必读:这是 no-op 拒绝而非真失败,说明目标内容已是期望状态或 old/new 复制成同一段,不要改用 bash 绕过
status: active
created: 2026-08-07
updated: 2026-08-07
source: inbox note 2026-08-07
---

错误原文: "old_string and new_string are identical — nothing to do"

含义: 这不是真正的失败,而是工具拒绝执行无变化替换——说明目标内容已是期望状态(改动早已生效)或构造替换时 old/new 复制成了同一段文本。

处理:
1. 先确认目标文件当前内容是否已是想要的版本;若是,直接跳过,无需任何补救。
2. 若确实需要改动,检查 old_string/new_string 是否笔误复制成相同文本,修正后重试。
3. 不需要改用 bash/sed 绕过——没有需要落盘的变化。

区别于 M-009 的 "old_string not found"(需 read 重读磁盘内容再精确匹配)。
