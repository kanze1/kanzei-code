---
id: M-013
scope: project
category: fact
title: git commit 报 exit code 1 且提示 Changes not staged 表示没有暂存内容
description: git commit 失败(exit code 1、"Changes not staged for commit")时必读:先检查同批前置 git add 是否已报 pathspec did not match;不能判定时只记症状,不要断言忘记 add。关联 D-159
status: active
created: 2026-08-08
updated: 2026-08-08
source: inbox note 2026-08-08 [fp:bash|exit code:]
---

git commit 报 exit code 1、输出 "Changes not staged for commit" / "Your branch is ahead of 'origin/dev'" 时:说明没有内容被暂存,但根因未必是忘记 git add。真实案例(D-159):同一 bash 调用中更早的 `git add <path>` 已报 `fatal: pathspec ... did not match any files`,因目标文件名不匹配导致 add 失败,commit 才显示无暂存。SOP:遇到 commit 无暂存,先检查同批前置 git add 是否失败(pathspec 错误/文件不存在);无法判定根因时只记录症状,不要断言用户忘记 add。确认 add 已成功仍报无暂存时,再按“改动未暂存,先 add 再 commit”处理。
