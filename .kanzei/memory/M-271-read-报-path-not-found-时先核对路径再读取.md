---
id: M-271
scope: project
category: sop
title: read 报 path not found 时先核对路径再读取
description: 处理 read 报 path not found 且同类错误复发时必读：先核对项目 memory 根目录和目标文件实际存在性，再读取确认存在的候选路径；禁止对失效原路径盲目重试，并记录路径映射以避免再次误读。
status: active
created: 2026-08-21
updated: 2026-08-21
source: memory-manager
---

适用场景：read 返回 `path not found`，尤其目标位于项目 memory 目录、疑似改名/移动，或同类错误已经复发时。

操作步骤：
1. 先确认项目 memory 根目录（本项目为 `\\?\\C:\\Users\\kanzei\\Documents\\kanzei code\\.kanzei\\memory`）；判断依据：不得把 `.kanzei\\memory` 下的文件误拼成不存在的路径。
2. 列出目标目录并核对候选文件的实际文件名/路径；判断依据：只有文件系统中确认存在的路径才可继续。
3. 对确认存在的候选路径执行 read，并在需要时记录旧路径到新路径的映射；判断依据：若文件已改名或移动，应修正引用而不是重试旧路径。

边界与例外：若目录本身不存在或候选文件均不存在，先定位正确的项目根/安装位置或确认文件是否已生成，再决定后续操作；不要把重复 read 当作修复。

复发指纹：[fp:read|path not found:]
