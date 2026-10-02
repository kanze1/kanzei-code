---
id: M-096
scope: project
category: sop
title: bash git merge blocked SOP
description: 处理 bash Git 合并时必读:Git mutations 必须用结构化工具(stage+commit);1-2 次复发记候选,第 3 次+修复成功用 episode 证直接晋升 active
status: deprecated
created: 2026-08-17
updated: 2026-08-18
source: memory-manager
---

判断要点：`git merge` in bash 被阻(需用 structured git tool)是环境契约错误;非一次性调试噪声。

验证证据：bash 明确提示"Git mutations must use the structured `git` tool",表明这是 Git 工具使用规范，非本次任务的临时问题。

(auto-deprecated: candidate 超出健康水位 24，按低价值优先清退；fingerprint=false，recurrence=0；原路径 C:\Users\kanzei\Documents\kanzei code\.kanzei\memory\M-096-bash-git-merge-blocked-sop.md)
