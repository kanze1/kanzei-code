---
id: M-271
scope: project
category: sop
title: read 报 path not found 时先核对项目 memory 根路径与目标文件
description: 处理 read/grep 报 `path not found`、尤其命中本条后同类仍复发时必读：不要依据推测的源码路径重试；先列出实际父目录并核对文件名/模块是否存在，再只读经目录证据确认的路径。最近邻候选不能证明目标文件存在。
status: active
created: 2026-08-21
updated: 2026-09-27
source: memory-manager
---

read 报 `path not found` 时，将未经验证的旧路径视为阻断条件；先核对实际项目根目录与父目录内容，再通过目录列表或 glob 确认目标文件的精确路径，取得证据前不得重试。工具提供的最近邻文件（如 codex.rs、mod.rs、store.rs）只表明同目录存在这些文件，不证明请求的目标文件存在；据此修正路径或确认目标缺失。\n\n复发指纹：[fp:read|path not found:]\n保留既有复发标记：[fp:bash|行动: 处理 read 报 path not found 或目标文件未找到时必读：先核对项目实际 memory 根路径，列出目标目录并确认精确文件名；若候选列表]\n[fp:bash|行动: 处理 read 报 时必读：把路径未证实视为阻断条件，先核对项目实际 memory]\n[fp:grep|path not found:]
