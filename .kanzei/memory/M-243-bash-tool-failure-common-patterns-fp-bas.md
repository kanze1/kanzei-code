---
id: M-243
scope: project
category: fact
title: bash tool failure common patterns [fp:bash|error]
description: 执行经验总结:bash 工具调用失败时的常见错误模式
status: candidate
created: 2026-08-18
updated: 2026-08-18
source: memory-manager
---

execution experience summary from recurring bash failures: 
1. "expected one of ... found": indicates bash is being called with invalid/unsupported parameters - often related to memory IDs like M-X requiring special handling. Root cause in D-495/D-519 was bash parameter construction errors for memory tools.
2. "unknown memory id M-X": indicates the memory id format is incorrect or the tool doesn't support that id directly.
3. "keyPathFailures": [] with notifications unpaired: related to PWA/desktop UI testing assertions requiring pairing state checks.

Key lesson: When bash fails, inspect stderr for error pattern and apply specific workaround (e.g., use defect tool for keyPath issues, verify memory ID format).
