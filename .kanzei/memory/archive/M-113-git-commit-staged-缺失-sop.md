---
id: M-113
scope: project
category: sop
title: git commit staged 缺失 SOP
description: 处理 git commit 失败时必读:Changes not staged for commit 必须先执行 git add 同批文件;4+ 次复发并有修复经验,确认为环境契约问题
status: deprecated
created: 2026-08-17
updated: 2026-10-07
source: user:note-2026-08-13
area: kanzei-tools/git
---

指纹：[fp:bash|行动: git commit 失败(exit code 、"Changes not staged for commit")时必读:先检查同批前置 git add], 原样放进正文——它是复发检测的键，丢了引擎就看不见「记了但没用」。

验证证据：4 次高复发 + 明确提到修复经验,表明这是 Git worktree 环境契约问题(commit 前需 staged),非一次性噪声。

(stale: 2026-10-07 用户授权项目记忆清理；源码复核：重复 Git 必须先 stage 的规则，当前结构化 Git 工具与 M-029 已覆盖；历史复发次数不是新的接口契约。)
