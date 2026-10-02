---
id: M-052
scope: project
category: sop
title: edit 替换失败 SOP:净删或顶档导致旧内容消失的防误操作准则与修复流程
description: 处理 edit 替换导致净删内容消失时必读：re-read target file，确认 old_string 是否被顶掉；若新增文本但历史未保留→先 full retain original content first;仅 confirm removal needed时才设 allow_deletion=true.多场景复发判据相同 — 必须保持所有 fp:edit|...marker原样记录作为检测签名
status: deprecated
created: 2026-08-11
updated: 2026-08-12
source: memory-manager
superseded_by: M-027
---

> 墓碑:2026-08-12 库存合并,本条已并入 M-027(内容重复,原文保留供追溯)。
适用场景:defact update 导致净删内容或大段原文消失，怀疑「想插入未成功」而非真正替换时必读。先 re-read 确认 target file 当前状态；若发现新增文本但 old_string 被顶掉且未被保留→90%是误操作将插⼊意图实为删除；正确做法:1)full retain original content first;2)只有 confirm removal needed时才设 allow_deletion=true.
