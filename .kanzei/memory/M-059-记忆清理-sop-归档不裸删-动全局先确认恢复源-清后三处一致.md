---
id: M-059
scope: project
category: sop
title: 记忆清理 SOP:归档不裸删,动全局先确认恢复源,清后三处一致
description: 手动清理 .kanzei/memory 或 ~/.kanzei/memory 前必读:防数据永久丢失与索引悬空
status: active
created: 2026-08-13
updated: 2026-08-13
source: 会话 2026-08-13 U-001~004 永久丢失事故复盘
---

1. **条目退役一律移入同目录 archive/,不裸删文件**。索引(memory_hits/FTS)里挂着的 id 失去文件会报 MISSING;项目侧可从 git 恢复,全局侧在 2026-08-13 建仓前无任何恢复源——U-001~004 就是裸删后永久丢失的实例(FTS 已删、recalls 空、回收站空)。
2. **动 ~/.kanzei/memory 前确认 git 仓库在**(2026-08-13 起已 git init,main 分支);清理完成后立即 commit,让下一次 MISSING 警告的 "restore from git" 真正可执行。
3. **清理后核对三处一致**:INDEX.md 条目行、记忆页类别计数、MISSING 警告为零。删文件时同步删它的 INDEX 行,归档 candidate 时同步改计数行。
4. **改写既有条目警惕跨主题覆写**:title/description/body 必须同主题,不同主题开新条目。反例:M-016(三主题缝合,原文被删光)、U-005(title 讲 R-163、description 讲 edit 指纹)。
