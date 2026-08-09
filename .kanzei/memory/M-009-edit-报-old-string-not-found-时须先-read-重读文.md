---
id: M-009
scope: project
category: sop
title: edit old_string not found：先 read 重读并逐字符重造，禁止凭摘要拼接
description: 处理 edit 报“old_string not found”或“must match exactly, including whitespace”时必读：先 read 当前文件目标区块，按实际输出重建并核对 old_string；若错误指出 Closest line/目标不符，禁止凭摘要重试，改用更小且带上下文的精确匹配后再 edit。
status: active
created: 2026-08-07
updated: 2026-08-09
source: inbox 2026-08-07
---

处理 edit 报“old_string not found”或“must match exactly, including whitespace”时，先 read 当前文件的目标区块并以实际输出重建 old_string；逐字符核对路径、空格、换行、缩进、标点和不可见字符，确认只命中目标后再 edit，禁止凭摘要、旧输出或臆测拼接后重试。

本轮复发错误原文：old_string not found in C:\Users\kanzei\Documents\kanzei code\crates/kanzei-app/ui/16-settings.js — it must match exactly, including whitespace. Closest line in file: `const SETTINGS_FORM_IDS = [`。看到 Closest line 仅为相近行时，不要把它当作匹配成功；重新 read 并围绕真实目标构造唯一 old_string。
[fp:edit|old_string not found in — it must match exactly, including whitespace.]
