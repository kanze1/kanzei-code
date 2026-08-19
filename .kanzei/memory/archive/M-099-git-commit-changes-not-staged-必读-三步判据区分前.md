---
id: M-099
scope: project
category: sop
title: git commit "Changes not staged"必读：三步判据区分前置未加 vs 其他错误
description: 处理 git commit "Changes not staged for commit" 失败时必读：区分前置未加（Case1）还是其他错误，补全判据并保留 [fp] 标记
status: deprecated
created: 2026-08-17
updated: 2026-08-18
source: memory-manager
subject: git commit staged missing 判据
---

git commit 失败的三步判据：  
1) exit code=1 + "Changes not staged for commit" → 同批前置未 git add（而非忘记）→ action: check staged files before the same batch, run git add accordingly, then retry commit。  
2) "Changes rejected due to ..." → 冲突或已暂存部分不匹配 → 用 M-XXX 处理。  
3) （其他 exit code≠1 情况）→ 对应各自错误类型（如 Arc copy/mismatch 等）→ NOOP 除非复发≥2 次。  

错误原文：exit code: 1 [1/8] ... Changes not staged for commit
[fp:bash|行动: git commit 失败(exit code、"Changes not staged for commit")时必读：先检查同批前置 git add]

(auto-deprecated: candidate 超出健康水位 24，按低价值优先清退；fingerprint=true，recurrence=0；原路径 C:\Users\kanzei\Documents\kanzei code\.kanzei\memory\M-099-git-commit-changes-not-staged-必读-三步判据区分前.md)
