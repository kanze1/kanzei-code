---
id: M-081
scope: project
category: sop
title: bash 错误复用性判断 + 晋升规则
description: 处理 bash Git 错误时必读:区分不可重用的环境契约错误(如 Arc Copy/手动 ASCII 比对)与一次性调试噪声;1-2 次复发记候选,第 3 次+修复成功用 episode 证直接晋升 active
status: candidate
created: 2026-08-17
updated: 2026-08-17
source: user:note-2026-08-13
refs: D-283
subject: bash 错误复用性判断
---

判断要点：这是环境/工具契约类的可复用知识(如 Arc::Copy/Rust 特性),还是本次任务内的一次性噪声?是前者才建条目。

指纹：[fp:bash|> error[E]: manual case-insensitive ASCII comparison], 原样放进正文——它是复发检测的键，丢了引擎就看不见「记了但没用」。

晋升规则：第 2 次才建 candidate(未验证);第 3 次+且带修复成功证据时,用 memory_add 建条目后 memory_promote 带 episode 证据升 active。
