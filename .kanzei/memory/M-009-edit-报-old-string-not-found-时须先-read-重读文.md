---
id: M-009
scope: project
category: sop
title: edit 报 old_string not found + must match exactly:先重读再精确构造含 whitespace 与缩进—非唯一匹配勿设 replace_all
description: 处理 edit old_string not found 时必读:先 read 重读文件排版再精确构造—match exactly including whitespace;多处匹配勿用 replace_all 盲改，并识别换行缩进陷阱
status: active
created: 2026-08-07
updated: 2026-08-20
source: inbox 2026-08-07;2026-08-13 自 quarantine 原版恢复
<span class="highlight">[fp: edit|old_string not found in — it must match exactly, including whitespace.]</span>
---

[fp:edit|old_string not found in — it must match exactly, including whitespace.] / [fp:edit|old_string matches locations in make it unique with more context, or set replace]
edit 报 old_string not found / must match exactly including whitespace 时必读:先 read 重读磁盘实际内容再构造 old_string;报错自带 "Closest line in file" 提示揭示文件真实排版(多个 key 挤同一行、每行一个等)。复发判据:连续多文件/多轮仍报此错=old_string 与磁盘实际排版不匹配,每次动手前必先 read 该文件当前内容,勿凭记忆构造。匹配多处(N locations)时不要设 replace_all 盲批量替换,先 read 找唯一上下文收窄 old_string;仅在明确批量替换意图时才 replace_all。
本批 2026-08-13~16 多文件持续复发(docstore.rs/drive.rs/replay_eval.rs/07-events.js/store.rs/mod.rs 等),先 read 是硬前提。
