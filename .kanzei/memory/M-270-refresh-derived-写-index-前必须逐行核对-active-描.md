---
id: M-270
scope: project
category: fact
title: refresh_derived 写 INDEX 前必须逐行核对 active 描述
description: 处理 MemoryStore 刷新 INDEX.md、或发现 INDEX 与 active M-*.md 描述可能串号时必读：写入前逐行核对 id 与源文件 description；有任何不一致就失败并先修复源数据，禁止生成不一致索引。
status: active
created: 2026-08-20
updated: 2026-08-20
source: user
refs: D-568
---

D-568 修复约束：MemoryStore::refresh_derived 写入 INDEX.md 前，逐行核对 active 条目的 id 与对应 M-*.md 源文件 description；任一不一致立即失败，避免 FTS 与 INDEX 串号。
