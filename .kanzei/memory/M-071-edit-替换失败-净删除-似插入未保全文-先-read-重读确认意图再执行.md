---
id: M-071
scope: project
category: sop
title: edit 替换失败(净删除/似插入未保全文):先 read 重读确认意图再执行
description: 处理 edit 替换失败(报"这次替换看着像插入(新文本多了N行)却没保住 old_string 原文"或"净删除N行")时必读:这是想插入却误把匹配段顶掉,或想新增却匹配到了要保留的内容(与 M-027 同坑)。判据:凡 edit 报"看着像插入/净删除"提示,先 read 重读原文件核对意图——本意是插入就把原文整段原样写进 new_string;确要删就置 allow_deletion=true。R-214/R-215/R-208/R-213 多轮复发,每次 edit 前先确认 old_string 含要保留的原文。
status: candidate
created: 2026-08-16
updated: 2026-08-16
source: memory-manager
---

错误:替换报"这次替换看着像插入(新文本多了 N 行),却没保住 old_string 里的原文"或"这次替换净删除 N 行"——未被保留的原文会列出被顶掉/删除的行。

判据:凡报"看着像插入"或"净删除"提示,先 read 重读原文件核对意图:
1. 本意是插入:把 old_string 原文整段原样写进 new_string(在附近追加内容),不要删掉它。
2. 确实要替换/删除:置 allow_deletion=true 重来。
3. 若本意新增但匹配到不该动的位置:缩小 old_string 或改成插入式替换(把原文原样包含在 new_string 里)。

已多轮多次复发(R-214 edit×27、R-215 edit×29 等大量命中),可复用 pitfall。R-208/R-213 亦命中。改 edit 替换时务必先 read 核对 old_string 是否含要保留的原文。
