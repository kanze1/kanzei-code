---
id: M-211
scope: project
category: fact
title: D-518 编译/测试失败 SOP：从 Bash 错误到 Root Cause 验证链路
description: bash/cargo 编译测试失败复发处理必读：何时遇到类型不兼容或超时异常时
status: candidate
created: 2026-08-17
updated: 2026-08-17
source: memory-manager
---

适用场景：bash 编译失败（mismatched types/timeout）或测试 runner 多行输出异常时的标准化修复链路。
操作步骤:
1. work:启动线程，确认目标版本分支与 cargo 路径
2. collaboration_status:检测并发修改冲突，有冲突则中止并通知
3. git:拉取最新改动确保干净，无未提交/冲突再继续
4. grep/symbols/read:defect/edit/insert:按序定位问题函数、类型声明、文档、缺陷标记和补丁插入，判断：grep 是否指向具体异常位置，symbols 类型声明是否与调用者匹配
5. bash:test_record:req执行编译与测试，bash code=0 通过即停，否则转 test_record 获取根因输出。
边界与例外:
- rustc/cargo 路径或版本变更需回退基础镜像
- bash>60s 超时立即切 test_record 并记录新阈值
- 多测试失败首条 failure 为根因
指纹：[fp:bash|error|timeout] 作为复发检测键。
