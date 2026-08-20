---
id: M-021
scope: project
category: sop
title: Edit old_string not found + must match exactly:先重读再精确构造含 whitespace 与缩进 — 非唯一匹配勿设 replace_all，否则批量替换风险不可控
description: 处理 edit 报 "old_string matches N locations" 时必读:先 read 找唯一上下文收窄 old_string,勿设 replace_all 盲批量替换;仅在明确批量替换意图时才 replace_all。
status: deprecated
created: 2026-08-09
updated: 2026-08-20
source: inbox:2026-08-09
superseded_by: M-009
---

[fp:edit|old_string matches locations in make it unique with more context, or set replace] / 报 "old_string matches N locations ... make it unique with more context, or set replace_all=true"
edit 报 old_string 匹配 N 处时:先 read 找唯一上下文收窄 old_string,勿直接 replace_all(批量替换风险不可控);仅在明确批量替换意图时才 replace_all。此坑多文件、多轮复发(2026-08-13~16:memory/mod.rs/store.rs、context_overflow_recovery.rs、test 断言块等),先 read 是硬前提,每处动手前先确认匹配唯一性或显式承担批量替换后果。
