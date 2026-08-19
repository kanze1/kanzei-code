---
id: M-193
scope: project
category: fact
title: D-480(fixed)流程根因：R- refs 冲突导致 unknown id
description: D-480(fixed)流程根因：R- refs 冲突导致 unknown id
status: deprecated
created: 2026-08-17
updated: 2026-08-18
source: memory-manager
---

## 根因事实
D-480(fixed)流程根因：工作区 R- refs 冲突导致「unknown id」识别失败。具体为 req 步骤预期存在已知 R- ref（R-286, R-283-R-264, R-242-245, R-248-249, R-264, R-281, R-288），实际执行时出现「unknown id `R-216`」报错。

## 适用场景
当 lint 流程进入 req 步骤检测到未知 R- ref ID 或 ref 列表长度/内容异常时触发。

## 操作步骤
1. 在 req 步骤前检查本地分支状态与 remote collaboration_status；判断依据：是否存在「unknown id」类 ref 冲突或未承诺更改导致 lint 执行前状态不一致
2. 若发现未知 R- ref，优先使用 git checkout 或 reset 清理本地修订号差异，再进入 collab_status 检查
3. 避免在无 clean/reset 前提条件满足时直接执行 lint 同步步骤

## 边界与例外
- 适用于「多作者协作/分支合并/long-running lint」场景（非一次性感知式 lint）
- 若 req 检测到的是预期内的 R- ref 冲突（如历史遗留的脏状态），而非未知 ID，则仍需清洁后重试

(auto-deprecated: candidate 超出健康水位 24，按低价值优先清退；fingerprint=false，recurrence=0；原路径 C:\Users\kanzei\Documents\kanzei code\.kanzei\memory\M-193-d-480-fixed-流程根因-r-refs-冲突导致-unknown-id.md)
