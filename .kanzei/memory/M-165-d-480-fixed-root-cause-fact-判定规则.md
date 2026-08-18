---
id: M-165
scope: project
category: fact
title: D-495(fixed)/D-519流程根因确认：缺陷跟踪工具缺失与 bash 参数构造错误 — 跨轮复发验证与 SOP 修正机制 [fp:bash|error: expected one of ... found; fp:M-: ERROR unknown memory id]
description: D-495(fixed)的根因是什么？确认是否为可复用知识
status: active
created: 2026-08-17
updated: 2026-08-17
source: user
---

D-495(fixed)根因确认：本批次失败的共同根因是 bash 命令链中工具函数调用缺失与环境配置未同步（详见M-237：memory_get/memory_archive不存在于可用集）。复发模式验证：
1. 第1次bash执行cargo/rustc命令 → error: unknown tool/memory id指纹记录（fp:bash|...）
2. 第2次仍失败 → 证据累加至阈值，触发SOP修正流程（M-236正文详述步骤）
3. 第3次+修复成功（episode≥775）→ memory_promote升active状态

对比历史经验：M-165已记录"D-495(fixed)根因候选"条目但内容为空，本次通过失败链验证明确根因为工具API缺失 + bash参数构造错误。D-519相关流程（defects.md协作推进 SOP）应纳入M-236步骤2的边界说明中，无需增条目。

**引用失败链证据**: [fp:bash|error: expected one of , , , , , or an operator, found] + [fp:M-: ERROR unknown memory id] + M-145读文件失败复发验证
