---
id: M-179
scope: project
category: fact
title: Bash Assertion fail:HTML 静态文案缺失资源表映射
description: 处理bash AssertionError时必读：HTML静态文案嵌入内存控制面的注册表同步失败模式[fp:bash|AssertionError [ERR_ASSERTION]: HTML 静态文案未进入资源表: 记忆控制面]
status: candidate
created: 2026-08-17
updated: 2026-08-17
source: memory-manager
refs: R-286
---

bash 工具调用失败特征：AssertionError[ERR_ASSERTION]错误提示"HTML 静态文案未进入资源表:记忆控制面"。错误发生在生成文件 ui-lint-globals.json(含720个顶层标识符)时，该文件需与源码保持同步；同时触发24个ui/*.js按序执行（共2293次invoke），涉及需求/缺陷/目标/测试/历史列表渲染及10个主视图切换流程。

本质是静态文案注入到资源表的契约违反——bash 作为工具调用者要求输入满足特定 schema，但实际传入的 HTML 内容结构或不完整导致断言失败。复发自如发生在跨轮次计数第1次即出现此类错误时（非一次性噪声）。
[fp:bash|AssertionError [ERR_ASSERTION]: HTML 静态文案未进入资源表:记忆控制面]

晋升前候选：待第2次观察到相同特征再建 entry，第3次及以上且有成功修复证据时用 episode_evidence promotion。
