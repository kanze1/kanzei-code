---
id: M-014
scope: project
category: fact
title: edit 锚点不匹配时重读原文，保留精确空白
description: 处理 edit 报 old_string not found 时必读:先 read 重读文件排排版再精确构造——match exactly including whitespace;多处匹配勿用 replace_all 盲改，保留 [fp:edit|old_string not found...]指纹
status: deprecated
created: 2026-08-08
updated: 2026-10-07
source: memory-manager
area: kanzei-tools/edit
---

编辑旧字符串不存在时必读：先 read 重读文件排版再精确构造 old_string — match exactly including whitespace;多处匹配勿用 replace_all 盲改。

**核心判据**: edit 报错 "old_string not found" 时，错误信息会明确说 "it must match exactly, including whitespace" —— 这是关键信号。此时不能直接用旧版本代码/注释/缩进拼 Old String，必须：

1. **先 read 重读原文件**（尤其是改过动的文件）
2. **逐字符核对**: tab/space 计数必须匹配；多行 old_string 的行数、每行开头空位数、特殊字符都必须原样复制
3. **避免 replace_all**: 多处可能的 match 时用 locate 精确定位，或在确认唯一性后再设 [replace_all=true]

**错误示例 (已复发验证)**: 用注释版代码构造 old_string 会因 tab/spaces 差异失败。

[fp:edit|old_string not found in — it must match exactly, including whitespace. Closest line in file: `                            let results = rt.background_results.c
]

(stale: 2026-10-07 用户授权项目记忆清理；源码复核：重复 M-009 的精确文本匹配与重读规则，保留 M-009 为当前项目记录。)
