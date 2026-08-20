---
id: M-253
scope: project
category: habit
title: cargo 编译 unexpected closing delimiter 复发排查 — 花括号/隐藏字符陷阱识别
description: 何时 recall: 遇到 cargo 编译「unexpected closing delimiter」错误时的跨轮排查 — 检查花括号配对/隐藏字符等环境契约问题
status: deprecated
created: 2026-08-18
updated: 2026-08-20
source: memory-manager
superseded_by: M-258
---

[fp:bash|error: unexpected closing delimiter:] - env/tool contract reusable knowledge：cargo 编译期「unexpected closing delimiter」错误通常由文件内花括号 nesting mismatch or trailing comment cause，修复路径：定位报错行号→检查该行前后花括号配对→确认无隐藏字符；若反复同错码复发(≥2轮)，记为已知坑位
