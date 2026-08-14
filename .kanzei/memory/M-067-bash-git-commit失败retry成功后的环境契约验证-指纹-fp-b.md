---
id: M-067
scope: project
category: fact
title: bash/git-commit失败retry成功后的环境契约验证—指纹[fp:bash|git-commit-failure-after-retry-success-and-blocked-or-no-staged];本记录覆盖exit code＝0+BLOCKED AND ROLLED BACK，退出码1+"Changes not staged"场景；成功后必须check前置commit操作
description: 处理 bash git commit 退出码＝0/BLOCKED AND ROLLED BACK + "Changes not staged"或exit-code-1+Changes not staged后retry成功:指纹[fp:bash|git-commit-failure-after-retry-success-and-blocked-or-no-staged]-必讀時確認環境/工具契約可複用知識，先check前置commit操作(防止再次block rollback)。本记录覆盖：Exit code＝0 + "BLOCKED AND ROLLED BACK... changes modified", Exit-code-1 + "Changes not staged for commit"
status: candidate
created: 2026-08-13
updated: 2026-08-13
source: memory-manager
---

bash/git commit 报错退出码＝0 + "BLOCKED AND ROLLED BACK... Changes modified files under .kanzei/project",以及exit-code-1 +"Changed snotstaged for commit"(git add遗漏导致)。指纹:[fp:bash|git-commit-failure-after-retry-success-and-blocked-or-no-staged]。判断要点:这是环境/工具契约类的可复用知识(第3次复发且跨轮计数确认),还是本次任务内的一次性噪声?是前者才建条目;后者判NOOP。行动点:重试成功后必须执行该check前置commit操作——防止误以为无需检查就继续，导致再次触发block rollback或重复提交尝试。
