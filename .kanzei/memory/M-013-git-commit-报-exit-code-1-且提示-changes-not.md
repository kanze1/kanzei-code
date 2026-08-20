---
id: M-013
scope: project
category: fact
title: 处理 edit 替换失败/换行符问题：先 read 重读再改
description: 处理 edit 替换失败/换行符问题时必读:先 read 重读再改
status: active
created: 2026-08-08
updated: 2026-08-20
source: inbox note 2026-08-08 [fp:bash|exit code:]
---

处理 bash/git commit 失败必读：先 check 缺失的 git add — Git commit 失败(exit code 1、\"Changes not staged for commit\")时必读:先检查同批前置 git add — 不能断言用户忘记，只记症状如路径未匹配。
处理 read 系统找不到文件失败时必读：先用 grep/glob 核实真实路径和文件名，再 read；路径不存在就停止，不要重复尝试。常见于：未用绝对路径、环境变量未刷新、CWD 错位、相对路径基准不同。错误原文：cannot open \\?\C:\Users\kanzei\Documents\kanzei code\crates\kanzei-memory\src\validation.rs: 系统找不到指定的文件。 (os error 2)
req 请求返回 unknown id: 对比已知有效 ID 列表找出差异 ID。错误原文：unknown id `D-569`; existing: R-283, R-284, R-285, R-287, R-101, R-245, R-248, R-249, R-264, R-281, R-288, R-296, R-299, R-307, R-308, R-309, R-310, R-311, R-312, R-313, R-314, R-315, R-316
[fp:bash|行动:git commit 失败(exit code、"Changes not staged for commit")时必读：先检查同批前置 git add]
[fp:read|cannot open 系统找不到指定的文件。 (os error )]
[fp:req|unknown id ; existing: R-, R-, R-, R-, R-, R-, R-, R-, R-, R-, R-, R-, R-, R-, R]
