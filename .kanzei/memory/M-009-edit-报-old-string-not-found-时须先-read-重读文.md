---
id: M-009
scope: project
category: sop
title: edit old_string not found 时必须 read 重读并按实际空白重建
description: 处理 edit 报“old_string not found”或“must match exactly, including whitespace”时必读：停止凭摘要重试，先 read 当前文件目标区块，按实际输出逐字符重建并用更小且带上下文的唯一匹配；若出现 Closest line，按提示重新定位后再 edit。
status: active
created: 2026-08-07
updated: 2026-08-10
source: inbox 2026-08-07
---

处理 edit 报 old_string not found 时，先 read 当前文件目标区块，按实际输出逐字符核对空白、缩进和换行后重建 old_string；禁止凭摘要或旧内容拼接重试。若错误给出 Closest line，说明目标文本/位置不符，应缩小匹配并加入文件路径、函数或邻近行上下文后再 edit。\n\n复发错误原文：old_string not found in C:\\Users\\kanzei\\Documents\\kanzei code\\crates/kanzei-app/ui/02-i18n.js — it must match exactly, including whitespace. Closest line in file: `  "切换到架构浏览": "Switch to architecture browser",`\n[fp:edit|old_string not found in — it must match exactly, including whitespace.]
