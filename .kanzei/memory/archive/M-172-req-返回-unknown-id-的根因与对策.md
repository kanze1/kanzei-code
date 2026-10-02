---
id: M-172
scope: project
category: fact
title: req 返回 unknown id 的根因与对策
description: 处理 req 报错提示 unknown id 时的必读：判断工具是否遵守已知 ID 契约
status: deprecated
created: 2026-08-17
updated: 2026-08-18
source: memory-manager
---

[fp:req|unknown id ; existing: R-, R-, R-, R-, R-, R-, R-, R-, R-, R-, R-, R-, R-, R-, R] req ID 不存在提示工具契约失效：当执行第 7 步检查已知需求时若返回 unknown id `R-216`或类似模式且存在列表包含 R-286/R-283 等有效 ID，说明 req 工具未将对应需求注册到系统——这是环境约束导致的系统性契约失败，需先通过 git create req 创建缺失条目再重试。

(auto-deprecated: candidate 超出健康水位 24，按低价值优先清退；fingerprint=true，recurrence=0；原路径 C:\Users\kanzei\Documents\kanzei code\.kanzei\memory\M-172-req-返回-unknown-id-的根因与对策.md)
