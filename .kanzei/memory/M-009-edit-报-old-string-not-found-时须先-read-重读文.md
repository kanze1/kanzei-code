---
id: M-009
scope: project
category: sop
title: edit old_string not found：先 read 重读并逐字符重造，禁止凭摘要拼接
description: 处理 edit 报“old_string not found”或“must match exactly, including whitespace”时必读：先 read 当前文件的目标区块并以实际输出重建 old_string；逐字符核对路径、空格、换行、缩进、标点和不可见字符，确认只命中目标后再 edit，禁止凭摘要、旧输出或臆测拼接后重试。
status: active
created: 2026-08-07
updated: 2026-08-09
source: inbox 2026-08-07
---

处理 edit 报错：`old_string not found in C:\Users\kanzei\Documents\kanzei code\crates/kanzei-app/src/run.rs — it must match exactly, including whitespace.`

决策判据：一旦出现该错误，立即停止继续 edit；重新 read 同一文件的目标区块，从 read 的原文复制/重建 old_string，逐字符核对路径、空格、换行、缩进、标点及不可见字符，并确保只命中目标后再执行 edit。不得使用摘要、旧 read 输出或手工猜测来拼接匹配串。

[fp:edit|old_string not found in — it must match exactly, including whitespace.]
