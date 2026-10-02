---
id: M-100
scope: project
category: sop
title: bash中git操作被拒绝：Git禁止bash修改—用结构化工具执行(先git stage指定文件并审查,再用git commit传入hash; fast-forward合并用git merge_ff)
description: 处理bash中git merge或整写操作被环境拒绝时必读：Git禁止bash做修改，必须用结构化git工具—先用git stage指定文件并审查staged_hash/diff，再用git commit传入hash，fast-forward合并用git merge_ff
status: deprecated
created: 2026-08-17
updated: 2026-08-18
source: memory-manager
---

[fp:bash|is blocked in bash: Git mutations must use the structured tool. Use with explicit files, review its staged_hash/diff, then git commit with that hash. Fast-forward merges go through git merge_ff (from/into; ) bash 整写或git merge命令被拒绝(exit code:1)时必读：当前环境禁止通过bash做Git修改，必须用结构化git工具替换——使用 git stage 指定文件并审查 staged_hash/diff，再用 git commit 传入该 hash，fast-forward合并需用 git merge_ff

(auto-deprecated: candidate 超出健康水位 24，按低价值优先清退；fingerprint=false，recurrence=0；原路径 C:\Users\kanzei\Documents\kanzei code\.kanzei\memory\M-100-bash中git操作被拒绝-git禁止bash修改-用结构化工具执行-先git.md)
