---
id: M-257
scope: project
category: sop
title: git 批量写入失败：shell bypass guard 阻断时的正确操作路径
description: 处理 shell 绕过编辑工具权限拒绝时必读：改用 edit 进行逐行修改·已强化判据以防复发
status: deprecated
created: 2026-08-18
updated: 2026-08-19
source: memory-manager
---

处理 git commit 批量写入失败时必读：shell(Set-Content) 被 guard `full-file-write` 阻断，因 shell bypass 跳过 edit/write工具的语法检查与 diff 展示。适用场景 + 操作步骤:\n适用场景：通过 bash 命令直接写文件触发 permission denied by guard `full-file-write: is blocked`\n(1) 错误判断：guard 阻止的是“whole-file rewrites via shell bypass”，而非路径权限问题\n(2) 正确操作：使用 edit工具进行 targeted line-level change (tolerates line-ending differences)\n(3) 边界例外：edit 仍被阻时，检查是否跨了 workspace 或 repo guard 范围；否则返回 NOOP。

(auto-deprecated: candidate 超出健康水位 24，按低价值优先清退；fingerprint=false，recurrence=0；原路径 \\?\C:\Users\kanzei\Documents\kanzei code\.kanzei\memory\M-257-git-批量写入失败-shell-bypass-guard-阻断时的正确操作路径.md)
