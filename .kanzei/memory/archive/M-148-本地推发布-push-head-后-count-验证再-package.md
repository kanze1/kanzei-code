---
id: M-148
scope: project
category: sop
title: 本地推发布：push HEAD 后 count 验证再 package
description: 处理发版时发现本地与远端版本不一致时必读:先 head push+count 再 package，否则 build 标签与实际提交数不匹配导致应用内版本错乱
status: deprecated
created: 2026-08-17
updated: 2026-08-18
source: memory-manager
refs: scripts/release.ps1 scripts/package.ps1
---

适用场景：本地发版后需同步推送并发布远端 GitHub Release，不能仅做本地安装。

操作步骤：
1. 先 push 当前 HEAD 到远程仓库（确保本地发版可追溯）
2. 按 build 标签以来的实际提交数运行 `package.ps1 -Ack <count> -Publish`  
3. 核对应用内"最新发布"是否对应本次 build

异常证据：用户曾截图显示本地 d49b2b92 与远端 build-e8aa005e 不一致，说明未 push 直接 package 会导致版本不匹配。

约束条件：本地 release 安装≠远端发布；远端发布必须有 HEAD push + count 验证链。

(auto-deprecated: candidate 超出健康水位 24，按低价值优先清退；fingerprint=false，recurrence=0；原路径 C:\Users\kanzei\Documents\kanzei code\.kanzei\memory\M-148-本地推发布-push-head-后-count-验证再-package.md)
