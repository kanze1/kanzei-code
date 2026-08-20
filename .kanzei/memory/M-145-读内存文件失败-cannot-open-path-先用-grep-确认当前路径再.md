---
id: M-145
scope: project
category: fact
title: 处理 read 系统找不到文件失败: 先 grep 核实路径再读
description: 处理 read 系统找不到文件失败时必读: 先用 grep/glob 核实真实路径和文件名，再 read；路径不存在就停止，不要重复尝试。常见于: 未用绝对路径、环境变量未刷新、CWD 错位、相对路径基准不同
status: active
created: 2026-08-17
updated: 2026-08-20
source: note 2026-08-17 [fact]
---

处理 read/edit/insert 报 `cannot open`、`系统找不到指定的路径` 或 `os error 3` 时：先用 grep/glob 核实当前工作区的真实路径和文件名；确认存在后再 read，确认不存在则修正路径或停止该路线，不要重复 read 同一路径，也不要改用无关工具绕过。
复发指纹：[fp:read|cannot open 系统找不到指定的文件。 (os error )]
复发指纹：[fp:read|不能打开系统找不到指定文件.os error ]
复发指纹：[fp:read|cannot open 系统找不到指定的路径。 (os error )]
证据：读取记忆文件时曾因路径不存在收到 `cannot open ... 系统找不到指定的路径。 (os error 3)`。
