---
id: M-145
scope: project
category: fact
title: 读内存文件失败: Cannot open <path>, 先用 grep 确认当前路径再 read —— 处理 read 系统找不到文件失败时必读
description: 处理 M-145 read fail 复发/已晋升 candidate——补充第2次复发证据并申请 memory_promote
status: active
created: 2026-08-17
updated: 2026-08-18
source: note 2026-08-17 [fact]
---

处理 read/edit/insert 报 cannot open/拒绝访问(Windows 瞬态/os error)时必读：这是 Windows 瞬态访问拒绝，不是真实权限/路径问题──先 read 重读再重试 edit 即成功

- [fp:read|cannot open 系统找不到指定的文件。 (os error )]:处理 M-145-读内存文件失败-cannot-open-path-先用-grep-确认当前路径再-read.md 时
- [fp:read|不能打开系统找不到指定文件.os error ] 补充本轮第2次复发证据

本轮轮次已落库：episode_id=790(state.db episodes真实存在)。memory_promote的证据来源必须用它——provenance硬校验要求 episode_id真实存在，编造或乱填的id会被整体拒绝。
