---
id: M-181
scope: project
category: fact
title: Req 请求信息 ID 不匹配（unknown id）
description: Req 步骤 unknown id 与现有 R-xxx 列表不匹配导致失败的模式
status: candidate
created: 2026-08-17
updated: 2026-08-17
source: memory-manager
---

## 失败特征
- **错误现象**: exit code: 1, error type: unknown id `R-216`; existing: R-286, R-283, R-284, R-285, R-287, R-235, R-101, R-242, R-243, R-245, R-248, R-249, R-264, R-281, R-288
- **触发条件**: req 步骤传入的 ID（如 R-216）不在现有 R-xxx 列表中的任何条目里

## 复发规律
- **发生场景**: workflow 中执行需求/资源请求信息获取阶段（req）时，使用的 ID 与系统现有资源池（R- 系列条目）不匹配
- **跨轮复发**: 后续同类任务若仍使用不匹配的 ID 必重复失败 → 需先核对列表再重试

## 根本原因
Req 模块在执行信息查询/请求流程前，必须先校验输入 ID 是否为已注册的资源条目。未校验直接传入未知 ID 导致整个 workflow 中断。

## 修复方向
1. **ID 预验证**: 在发送 req 前先用 grep 扫描现有 R-xxx 列表，确认目标 ID 存在性
2. **动态比对**: 将待操作 ID 与已知列表逐一比对，若"unknown id X; existing: Y,Y,Y..."则说明 ID 错误
3. **修正策略**: 使用从 grep 结果中解析出的正确 ID（如 R-286）替换原未知 ID 后重试 req 步骤
