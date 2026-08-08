---
id: M-018
scope: project
category: sop
title: 发版流程(scripts/package.ps1):捕获 git 输出须先切 UTF-8;gh release create 的 target 须已 push 到 origin
description: 处理发版 / gh release create 报 HTTP 422 "target_commitish is invalid"、或 package.ps1 D-183 区间核对提交数偏少/中文提交信息吞行合并、发布被误拦时必读
status: active
created: 2026-08-08
updated: 2026-08-08
source: 发版 build-ccfecff, 2026-08-08
---

scripts/package.ps1 发版前置约束:
① D-183 区间核对:捕获 git log 输出前必须先 `[Console]::OutputEncoding=UTF8`,否则 PowerShell 按系统代码页(如 GBK)解码 UTF-8 中文提交信息会吞行合并(实测 6 个提交被判成 5,发布被误拦)。该修复已随 commit ccfecff 进脚本。
② `gh release create --target` 需要 40 位完整 SHA,且该提交必须已 push 到 origin;本地领先 origin 时直接发版报 HTTP 422 "target_commitish is invalid"。
③ 正确顺序:先 `git push origin dev`,再 `gh release create`。本次发版 build-ccfecff 即按此流程。
