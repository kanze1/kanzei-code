---
id: M-009
scope: project
category: sop
title: edit 报 old_string not found 时须先 read 重读文件再精确匹配
description: 处理 edit 替换失败(old_string not found / must match exactly including whitespace)时必读:先 read 重读磁盘实际内容再构造 old_string;报错自带 "Closest line in file" 提示会直接揭示文件真实排版(多个 key 挤同一行 vs 自己按每行一个 key 构造),按该排版重造 old_string。已复发于 main.rs 的 #[test] 缩进(2 次)。
status: active
created: 2026-08-07
updated: 2026-08-13
source: inbox 2026-08-07;2026-08-13 自 quarantine 原版恢复
---

[fp:edit|old_string not found in — it must match exactly, including whitespace.]
错误原文: "old_string not found in <file> — it must match exactly, including whitespace. Closest line in file: `    #[test]`"(main.rs)。另一实例:main.js 中 "外部阻塞"/"阻塞"/"可执行"/"阻塞原因" 等 i18n 键实际在同一行,按每行一个 key 构造 old_string 必不命中。
处置:不重试同一 old_string;先 read 重读磁盘实际排版,按 "Closest line in file" 提示的真实排版重造 old_string,再 edit。复发 2 次证明此判据必须进入决策。

恢复记录(2026-08-13):本条正文曾于 08-11/12 被弱模型改写成幻觉内容(before_replace_hook/read_file_at_line 均不存在)后退役;本文以 08-09 原版为准恢复,坏版本保留在 archive 供追溯。本条历史采纳率为全库最高(24/54),edit 失败也是事件召回第二大触发源,必须保持 active。
