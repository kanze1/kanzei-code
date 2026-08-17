---
id: M-085
scope: project
category: sop
title: Git commit failure patterns (add check/structured tool)
description: 处理 Git 修改失败时必读:检查前置 git add 状态，使用结构化工具而非bash直接执行
status: candidate
created: 2026-08-17
updated: 2026-08-17
source: memory-manager
---

Git commit 失败常见模式：

1. "Changes not staged for commit" - 检查同批前置是否执行了 git add，批量提交必须逐文件先阶段化
2. Bash直接修改报错 "Git mutations must use the structured tool" - 必须使用`git stage`+显式文件，审查staged_hash/diff后`git commit`用相同hash

重复规则：第1次失败→记录模式；第2次观察；第3次+修复成功→记忆。指纹[fps:bash|error: Changes not staged...]必须保留作为复发检测键。
