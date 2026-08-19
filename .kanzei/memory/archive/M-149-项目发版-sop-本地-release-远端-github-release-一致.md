---
id: M-149
scope: project
category: sop
title: 项目发版 SOP：本地 release+远端 GitHub Release 一致性与发布流程
description: 处理 X 项目发版时必读：确保本地安装与远端 GitHub Release 一致，含 push HEAD 与 package.ps1 参数；[fp:sop] 缺失则触发重复错误
status: deprecated
created: 2026-08-17
updated: 2026-08-18
source: memory-manager
---

适用场景：项目发版后必须同步推送并发布远端 GitHub Release，不能只做本地安装。操作步骤：1) 用当前 HEAD 执行 git push（远端发布必须先 push 当前 HEAD）；2) 计算自上次 build 标签以来的实际提交数 count；3) 运行 package.ps1 -Ack <count> -Publish（按实际提交数执行，不可硬编码假计数）；4) 核对应用内"最新发布"对应本次 build（确保本地 d49b2b92、远端 build-e8aa005e 一致）。边界与例外：若本地/远端不匹配则 SOP 未正确执行或计数错误；package.ps1 必须带 -Ack <count> -Publish 参数，仅 -Ack 无发布是局部错误；应用内"最新发布"核对应最终一致性校验。引用：scripts/release.ps1（项目特有发版规则）、scripts/package.ps1。

(auto-deprecated: candidate 超出健康水位 24，按低价值优先清退；fingerprint=false，recurrence=0；原路径 C:\Users\kanzei\Documents\kanzei code\.kanzei\memory\M-149-项目发版-sop-本地-release-远端-github-release-一致.md)
