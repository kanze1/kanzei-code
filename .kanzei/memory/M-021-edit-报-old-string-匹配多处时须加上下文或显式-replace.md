---
id: M-021
scope: project
category: sop
title: edit 报 old_string 匹配多处时须加上下文或显式 replace_all=true
description: 处理 edit 报 "old_string matches N locations ... make it unique with more context, or set replace_all=true" 时必读:先 read 重读确认目标串出现次数,优先加更多上下文让 old_string 唯一;只有确认要整批替换才设 replace_all=true,不要盲开 replace_all 误改其它位置。
status: active
created: 2026-08-09
updated: 2026-08-09
source: inbox:2026-08-09
---

错误原文: old_string matches 28 locations in C:\Users\kanzei\Documents\kanzei code\crates/kanzei-app/src/update_tests_update.rs; make it unique with more context, or set replace_all=true.
实例(2026-08-09): update_tests_update.rs 中目标串出现 28 处,edit 失败后改用 read 重读、缩小 old_string 范围/加上下文成功。
[fp:edit|old_string matches locations in make it unique with more context, or set replace]
