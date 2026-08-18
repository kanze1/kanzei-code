---
id: M-184
scope: project
category: fact
title: Git fatal:Needed a single revision 失败模式
description: 处理 git 工具调用失败分析：确认是否环境/契约类知识（重复模式、明确错误特征）还是一次性噪声；前者需建 entry。
status: candidate
created: 2026-08-17
updated: 2026-08-17
source: memory-manager
---

git 工具调用失败特征：fatal:"Needed a single revision"错误发生在--count-all 场景。本质是 git 仓库状态或分支结构存在冲突——可能需要合并、重置或回退某个操作才能产生有效的 rev 标识，否则无法完成计数或统计类命令执行。

复发场景：git 相关工具（checkout/merge/rebase/count等）在分支未就绪或历史混乱时出现；属于环境约束类问题（非任务内一次性误用）。

晋升前候选：待第2次观察到相同特征再建 entry，第3次及以上且有成功修复证据时用 episode_id=696 进行 promotion。
