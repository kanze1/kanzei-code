---
id: M-145
scope: project
category: fact
title: 读内存文件失败: Cannot open <path>, 先用 grep 确认当前路径再 read
description: read 失败时必读:系统找不到文件需用 grep 确认当前路径
status: candidate
created: 2026-08-17
updated: 2026-08-17
source: note 2026-08-17 [fact]
---

环境契约事实：文件路径不存在会导致 read 工具报错 "cannot open <path>: 系统找不到指定的文件 (os error 2)"。可能原因：工作空间路径变更、相对路径未更新或项目结构变动导致文件移动/删除。解决方式：先用 grep/status 确认文件当前实际位置，再读取正确路径。

指纹用于复发检测: [fp:read|cannot open 系统找不到指定的文件。 (os error )]
