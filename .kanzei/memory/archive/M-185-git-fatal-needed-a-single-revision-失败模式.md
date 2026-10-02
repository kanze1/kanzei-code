---
id: M-185
scope: project
category: fact
title: Git fatal:Needed a single revision 失败模式
description: 处理git工具调用失败分析：判断是否环境/契约类知识；前者建条目用episode_evidence promotion
status: deprecated
created: 2026-08-17
updated: 2026-08-18
source: memory-manager
---

git工具失败特征:fatal:"Needed a single revision"发生在--count-all场景。本质是仓库状态/分支冲突——需合并重置或回退操作产生有效rev标识。复发于checkout/merge/rebase/count等命令在分支未就绪时出现，属环境约束问题。

证据指纹:[fp:tool|fatal: Needed a single revision]
晋升：episode_id=696待3次后promotion。

(auto-deprecated: candidate 超出健康水位 24，按低价值优先清退；fingerprint=true，recurrence=0；原路径 C:\Users\kanzei\Documents\kanzei code\.kanzei\memory\M-185-git-fatal-needed-a-single-revision-失败模式.md)
