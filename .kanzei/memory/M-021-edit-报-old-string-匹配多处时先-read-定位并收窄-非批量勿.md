---
id: M-021
scope: project
category: sop
title: Edit old_string not found + must match exactly:先重读再精确构造含 whitespace 与缩进 — 非唯一匹配勿设 replace_all，否则批量替换风险不可控
description: old_string not found + must match exactly whitespace:先 read 重读磁盘实际内容再精确构造 old_string。报错自带 "Closest line..."提示揭示真实排版(多 key 挤同一行/按每行一个构造)。已复发于 main.rs #[test]缩进，判据含「未命中全文+whitespace strict」而非单次尝试
status: active
created: 2026-08-09
updated: 2026-08-13
source: inbox:2026-08-09
---

处理 edit 报错：`old_string matches 18 locations in C:\Users\kanzei\Documents\kanzei code\crates/kanzei-app/src/update.rs; make it unique with more context, or set replace_all=true.` 时，停止重试原字符串；先 read 当前目标区块，补入文件结构、函数/区块边界和足够邻近行，使 old_string 只命中目标 1 处，再执行 edit。仅在明确的批量替换且已核对所有命中范围时才设置 replace_all=true，不能用它掩盖定位不准。\n[fp:edit|old_string matches locations in make it unique with more context, or set replace]

恢复记录(2026-08-13):本条 08-12 被批量退役属误伤——事件日志显示该失败类近 3 日仍在复发,且历史采纳数据证明其决策价值;经用户指示的记忆清理恢复为 active。
