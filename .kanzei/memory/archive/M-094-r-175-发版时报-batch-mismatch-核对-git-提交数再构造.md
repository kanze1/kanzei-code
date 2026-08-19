---
id: M-094
scope: project
category: sop
title: R-175 发版时报 batch mismatch:核对 git 提交数再构造 release version
description: 处理发版批次号与 Git 历史不一致错误：运行前必须核对真实提交数再构造版本
status: deprecated
created: 2026-08-17
updated: 2026-08-18
source: memory-manager
---

处理 R-175 类发版时版本控制陷阱：R-175 的手写批次（如 6/6）与 Git 提交历史标记数必须一致 —— 若不一致会触发 "batch field mismatch" 阻塞。标准流程：打包前 run git log --oneline | wc -l 核对真实计数，再按此计数构造 release batch number;否则关闭时会失败。

(auto-deprecated: candidate 超出健康水位 24，按低价值优先清退；fingerprint=false，recurrence=0；原路径 C:\Users\kanzei\Documents\kanzei code\.kanzei\memory\M-094-r-175-发版时报-batch-mismatch-核对-git-提交数再构造.md)
