---
id: M-009
scope: project
category: sop
title: edit old_string not found：先读盘再按报错提示的精确内容重造
description: 处理 edit 替换失败(old_string not found/whitespace mismatch)必读：先读盘重读再按报错精确匹配，别凭记忆或 bypass
status: active
created: 2026-08-07
updated: 2026-08-09
source: inbox 2026-08-07
---

Edit 替换失败(old_string not found)。工具报 "it must match exactly, including whitespace"，Closest line in file: `        reasoning: ReasoningEffort::Off,`。

判据升级：不要凭记忆、不要改 bash/其他绕过；必须第一步 read 重读文件实际内容(含缩进换行格式)，严格照报错给出的 Closest line + 上下文重造 old_string，逐字精确匹配编辑目标串——否则必然不命中。

错误指纹 (关键检测码): [fp:edit|old_string not found in — it must match exactly, including whitespace.]
