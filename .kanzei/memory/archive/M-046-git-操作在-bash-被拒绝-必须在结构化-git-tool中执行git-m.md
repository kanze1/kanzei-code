---
id: M-046
scope: project
category: fact
title: git 操作在 bash 被拒绝 — 必须在结构化 git tool中执行Git mutation的固化知识 [fp:bash|...]
description: git restore在bash执行被引擎拒绝，必须用结构化 git 工具的固化知识:fp:bash|`git restore is blocked in bash:...`用于复发检测与防错提醒 — 第1次跨轮计数的工具契约类失败记录,后续重复将触发相同判据自动召回本条
status: deprecated
created: 2026-08-11
updated: 2026-08-12
source: memory-manager
refs: D-204
superseded_by: M-029
---

> 墓碑:2026-08-12 库存合并,本条已并入 M-029(内容重复,原文保留供追溯)。
【复发指纹】[fp:bash|`git restore is blocked in bash: Git mutations must use the structured `git` to] — 在bash环境尝试执行任何Git mutation命令(git add, git commit, git reset, git rebase等)时都会被引擎拒绝，提示需用结构化`git` tool。

【适用场景】在处理需要修改git仓库(添加、暂存、commit、reset、rebase等操作)的场景中：1) bash shell发出这些command直接报错;2) 之前误用bash运行Git操作导致失败但没记录此契约知识。

【操作步骤】
1.遇到任何` git <cmd>`在bash报"[git] mutations must use [the structured `git`]tool"错误，先停止所有bash命令执行该操作的企图;2)。使用引擎提供的结构化工具替代方案:对于add操作->用[git stage];需要确认staged_hash后commit时直接用那个hash值提交而非通过bash调用原command或任何等价变体。

【边界与例外】
——此错误是跨轮计数的第1次复发，属于可复用知识;fp标记确保后续类似误用时能立即识别并触发该SOP的召回机制:D-204等已有条目不应与此混淆(后者是关于defect field处理);bash/git工具使用契约类故障必须在记忆中留下指纹以便下次遇到时自动检索。
