---
id: M-125
scope: project
category: sop
title: git commit 'Changes not staged for commit' 必读：先执行 git add
description: M-102: git commit Changes not staged for commit必读 — 处理多次失败后的 git add 前置检查流程
status: deprecated
created: 2026-08-17
updated: 2026-08-18
source: memory-manager
---

适用场景：git commit 失败 (exit code 1 + "Changes not staged for commit"),尤其是多次重复失败时。操作步骤：先运行 git add <files...> 将变更加入暂存区，再 commit；检查是否有未 staging 的修改。边界与例外：若文件确无新增变更则跳过 commit(避免 create empty commit)；PS/PowerShell 下仍须用结构化 git tool（勿试 bash 绕过）。

(auto-deprecated: candidate 超出健康水位 24，按低价值优先清退；fingerprint=false，recurrence=0；原路径 C:\Users\kanzei\Documents\kanzei code\.kanzei\memory\M-125-git-commit-changes-not-staged-for-commit.md)
