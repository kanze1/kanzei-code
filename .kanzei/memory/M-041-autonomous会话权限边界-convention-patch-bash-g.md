---
id: M-041
scope: project
category: sop
title: autonomous 会话报 permission requires user approval 是档位限制,不是死路
description: 处理 autonomous(自动推进)会话里 edit/bash/git/cargo/conventions_patch 被拒并报 "permission requires user approval" 时必读:这是权限档位而非工具故障——把该动作留给交互轮或先在 .kanzei/kanzei.toml 加白名单;不要反复重试、不要换等价命令绕道、也不要判定为死路而放弃整条任务。
status: active
created: 2026-08-11
updated: 2026-08-12
source: memory-manager;2026-08-12 库存合并(三次同类复发)
supersedes: M-042 M-043
refs: D-239 R-157 R-164 D-259
---

**判据**:报错是 `permission requires user approval: <tool> on ...`,不是参数错、不是路径错。同一档位下重试多少次都是同一个结果。

**实测边界**(2026-08-11 autonomous 轮):`.kanzei/kanzei.toml` 白名单只放行了 style.css 的 edit 和少量精确 bash 命令;`conventions_patch` 被拒、`git stage` 被拒(结构化 git 工具同样受档位约束)、非白名单 bash(`Get-ChildItem` 组合、`git log --grep`)全部被拒。因此该档位下能做的只有:tracker 字段复核、只读代码勘察、记忆维护、test_record 收尾。

**正确反应**:
1. 停手,不要用等价命令换皮重试(换 `Select-String`、换 `git log --all` 一样被拒);
2. 把需要写权限的动作(源码修复、cargo 测试、git 提交)明确列出来,留给交互轮由用户批准;
3. 要长期放行就改 `.kanzei/kanzei.toml` 加白名单规则,而不是每轮碰一次墙;
4. 不要因为写不了就宣告任务无解——只读勘察和登记照样能推进。

**反面教训**:这条第一次记录后又原样复发两次,原因是当时的条目停在 candidate、召回和复发检测都只看 active,等于记了个寂寞。合并晋升为 active 就是修这个。

**注意**:这类错误无法靠 Tier0 指纹精确命中——引擎生成的 fingerprint 会把整条命令 JSON 拼进 kind(如 ``[fp:bash|permission requires user approval: bash on `{"command":"Get-ChildItem …]``),命令一变指纹就变,所以本条不写 fingerprint 字段,靠 description 的召回钩子和 Tier1 命中。
