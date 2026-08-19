---
id: M-180
scope: project
category: fact
title: Git 命令需要单 revision 参数失败（fatal error）
description: Git commit 需要单 revision 参数的fatal错误模式
status: deprecated
created: 2026-08-17
updated: 2026-08-18
source: memory-manager
---

[fp:bash|fatal: Needed a single revision]

## 失败特征
- **错误现象**: git 命令返回 fatal: Needed a single revision（exit code: 1, --count-all 模式下）
- **触发条件**: 执行需要明确 revision 标识的 git 操作时，当前状态/上下文未提供有效 revision

## 复发规律
- **发生场景**: 
  - 在缺乏显式 revision 参数（如 rev-parse, cherry-pick, revert 等涉及具体版本的命令）时必然触发
  - 当 repository 处于初始状态或分支结构不完整时高频出现
- **跨轮复发**: 第 1 次尝试即出现 → 后续同样缺失 revision 标识的操作必重复

## 根本原因
Git 在执行需要定位/切换至具体 revision 的命令前，必须先解析出唯一的 revision 标识；若当前工作区或历史状态下无法确定唯一分支/commit identifier，则拒绝执行并抛出 fatal 错误。

## 修复方向
1. **策略调整**: 改为"先 fetch --depth=1"→"rev-parse HEAD~N"确认单 revision →再执行目标命令，确保上下文中有明确标识
2. **参数补全**: 显式添加--no-verify-fetch、或使用 rev-parse --short --verify 等变通手段
3. **版本锁定**: 将所需 revision 硬编码到脚本参数中（如 commit:a1b2c3d）而非依赖环境状态识别

(auto-deprecated: candidate 超出健康水位 24，按低价值优先清退；fingerprint=true，recurrence=0；原路径 C:\Users\kanzei\Documents\kanzei code\.kanzei\memory\M-180-git-命令需要单-revision-参数失败-fatal-error.md)
