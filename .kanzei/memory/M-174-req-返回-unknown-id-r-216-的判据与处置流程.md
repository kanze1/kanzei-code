---
id: M-174
scope: project
category: fact
title: req 返回 unknown id[R-216]的判据与处置流程
description: 处理 req 返回 unknown id 时必读:如何判断是否为 ID 引用契约问题
status: candidate
created: 2026-08-17
updated: 2026-08-17
source: memory-manager
---

[fp:req|unknown id ; existing: R-, R-, R-, R-, R-, R-, R-, R-, R-, R-, R-, R-, R-, R-, R]

**判断标准**: req 返回 unknown id 是否属于可复用知识？
□是 (建条目): 
  - 需求ID本身不存在于系统(未定义/缺失配置)
  - 其他req调用也指向相同未知ID(系统级问题)
  - 需要创建新需求或修复ID引用的架构约束

□否,NOOP(本次为一次性噪声):
  - ID仅在当前会话/任务中存在但未持久化
  - 工具契约变更导致的临时状态
  - TDD环境下预期的编译失败后自我修正

**处置流程**:
1. 检查unknown id是否在现有要求库中引用(R-286等)
2. 确认是否存在相同ID的其他需求实例
3. 若是系统缺漏→创建新需求条目；若是引用错误→修正指向
4. 记录此模式的触发条件(配置、分支、环境差异)

**适用范围**: 
- ui-lint-globals.json生成流程
- D-480相关需求验证环节
- 跨项目/跨环境需求同步场景
