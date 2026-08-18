---
id: M-236
scope: project
category: fact
title: bash 编译工具链调用失败模式与递归修正机制 — 针对 cargo build/run error: expected one of `!` . `::` ? found 及 unknown memory id 的通用处置流程 [fp:bash|error: expected one of , , , , , or an operator, found; fp:M-: ERROR unknown memory id]
description: bash 编译错误复发检测与修复 — 当 bash 执行 cargo/rustc 命令报错时必读
status: candidate
created: 2026-08-17
updated: 2026-08-17
source: memory-manager
---

D-495(fixed)根因与复发模式总结（合并自M-165与本次失败链）

**适用场景**: bash执行cargo/rustc编译或运行命令时触发：
- 错误模式A::编译器内部语法解析失败（expected one of ! . :: ? found X，定位到源文件行号）→ 通常是工具参数/字符串构造错误引发编译错误树崩掉
- 错误模式B: unknown memory id M-X → ID占位符在代码模板未更新或缓存未刷新

**操作步骤**: 
1. read 编译失败栈顶源码文件对应行号的上下文（包含match/store等符号定义位置）→ 判断依据：编译器报错信息中的file:路径与线号
2. grep/inspect 确认该符号在当前环境配置下的合法语法格式 → 判断依据：cargo/rustc默认参数集或项目级config
3. match exact edit该行构造（保留缩进与注释）→ 判断依据：compiler内部语法表约束，不可用正则匹配
4. bash cargo build/run再次执行编译 → 判断依据：错误是否仍出现在相同位置，若消失则修复成功

**边界与例外**: 
- 若第2次失败仍出现同样错误模式，说明根因未触及深层参数/配置
- 工具调用链中memory_get/memory_archive不存在于当前环境可用集（见M-165/M-208）
- 权限拒绝模式fp:write需优先改用memory_note通道（M-005/M-235）

**引用失败链证据**: 
[fp:bash|error: expected one of , , , , , or an operator, found] (编译错误检测)
[fp:M-: ERROR unknown memory id] (ID解析器阻塞标记)
来源自：batch failure #1 (#2) + episode evidence 累积验证

**晋升路径**: 
- 第1次record → candidate状态
- 第2次仍复发 → 补充证据后memory_promote(episode_id=775)升active
