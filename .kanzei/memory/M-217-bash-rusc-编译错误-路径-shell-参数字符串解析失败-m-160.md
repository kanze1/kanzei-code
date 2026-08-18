---
id: M-217
scope: project
category: fact
title: bash rusc 编译错误：路径 shell 参数字符串解析失败 (M-160)
description: 处理 rustc/bash 编译错误(路径 shell 参数字符串解析失败):记录第3次失败带成功证据则 promote。
status: candidate
created: 2026-08-17
updated: 2026-08-17
source: run:2026-08-17
---

bash/rustc 编译运行错误：预期运算符解析失败 (M-160::line 2049)

触发条件(第3次+): M-160::match store 语句在 \\?\C:\Users\kanzei\Documents\kanzei code\crates\kanzei-memory\src\memory\mod.rs:2049:9 出现语法错误 - rustc 无法识别 `store` 作为匹配臂。第1-2次为候选记录;第3+次且修复成功则 promote。

根因：bash/rustc 命令行解析环境，Rust 编译器对 shell 参数空间/路径分隔符 (\\?\C) 处理差异导致的 token 解析失败(非 bash 本身问题)。

适用场景 + 操作步骤:
- 触发场景：cargo build/dev/profile 编译遇到 `expected one of ! . :: ? {` 且无法继续。
- 步骤①: 检查源码行号/路径，确认是否有转义字符或 shell 参数异常(\\?\C::)。
- 步骤②: git status 确认是否未提交(rustc 会提示 `git diff` 建议)。

边界与例外：非 bash/rustc 环境;仅针对 M-160 mod.rs 的 match arm 问题。

refs: R-204, R-165
