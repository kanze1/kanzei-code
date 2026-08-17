---
id: M-010
scope: project
category: sop
title: edit 报 old/new 相同是 no-op 拒绝而非失败
description: 处理 edit 报 "old_string and new_string are identical — nothing to do" 时必读:这是 no-op 拒绝(提交的 new==old 无改动),停止重试,先 read 确认目标是否已达成,未达成则让 new_string 与 old_string 不同,勿用 bash 绕过。
status: active
created: 2026-08-07
updated: 2026-08-16
source: inbox note 2026-08-07
---

[fp:edit|old_string and new_string are identical — nothing to do]
edit 报 "old_string and new_string are identical — nothing to do" 是 no-op 拒绝而非失败:你提交的 new_string 与 old_string 完全相同,没有改动。处理:停止重试,先 read 确认目标是否已是期望状态;若已完成则无需再改(记 active 即可),若未完成则修改 new_string 使其与 old_string 不同,不要用 bash 绕过(整文件重写会被 M-019 拦截)。
复发判据(2026-08-13~16 反复出现):一旦再遇 identical no-op,说明在重复提交未完成的替换——先 read 目标文件确认现状再构造 next_string,勿盲目重发相同字符串。
