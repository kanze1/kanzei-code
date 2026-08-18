---
id: M-170
scope: project
category: fact
title: fact:req now_ms函数模块作用域缺失约束
description: req反复失败后根因分析必读:何时判断函数定义缺失/模块作用域问题
status: candidate
created: 2026-08-17
updated: 2026-08-17
source: user:2026-08-17-fact-req
refs: R-070
---

根因类型: 函数/模块未能在预期作用域内找到定义,属于环境/工具契约类约束。具体问题: req尝试访问`now_ms`函数但该函数未在 super module 中正确声明或导出，导致编译错误[E0425]: cannot find function `now_ms` in module `super`。复发频率: 第2次出现(跨轮计数),表明这是已知模式而非偶发操作失误。影响范围: 涉及 cargo编译目标,会阻塞D-480修复流程的bash步骤执行。

指纹:[fp:req|unknown id ; existing: R-, R-, R-, R-, R-, R-, R-, R-, R-, R-, R-, R-, R-, R-, R]
