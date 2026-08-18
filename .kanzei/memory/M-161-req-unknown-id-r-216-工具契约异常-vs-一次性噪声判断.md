---
id: M-161
scope: project
category: fact
title: req unknown id R-216:工具契约异常 vs 一次性噪声判断
description: 处理 req 反复返回 unknown id 时必读:判断是环境契约问题还是一次性噪声(跨轮重复复发则为环境约束)
status: candidate
created: 2026-08-17
updated: 2026-08-17
source: memory-manager
---

req 返回 unknown id `R-216`:existing 条目固定为 R-286,R-283,R-284,R-285,R-287,R-235,R-101,R-242,R-243,R-245,R-248,R-249,R-264,R-281,R-288。跨轮重复出现表明是工具/环境契约问题，非一次性噪声。指纹:[fp:req|unknown id ; existing: R-286,R-283,R-284,R-285,R-287,R-235,R-101,R-242,R-243,R-245,R-248,R-249,R-264,R-281,R-288]
