---
id: M-271
scope: project
category: sop
title: read 报 path not found 时先核对项目 memory 根路径与目标文件
description: 处理 read/grep 报 `path not found`，特别是检查过记忆仍复发时必读：先列出实际父目录并确认目标文件存在；若父目录无可列出文件，停止沿错误文本中的路径或最近邻候选重试，转而查真实目录、生成步骤或路径配置。
status: deprecated
created: 2026-08-21
updated: 2026-10-24
source: memory-manager
area: kanzei-tools/read
---

处理 read/grep 报 `path not found`，先核对真实父目录和目标文件是否存在；最近邻候选不是存在性证据。若父目录为空/不可列出，停止重复读取猜测路径，查真实目录及生成步骤。本条已存在但同类错误仍复发，强化此判据。
[fp:read|path not found:] 错误示例：path not found: C:\Users\kanzei\Documents\kanzei code\output/harness-release-2026-10-02/verification.json；同目录没有可列出的文件。
[fp:bash|行动: 处理 read 报 path not found 或目标文件未找到时必读：先核对项目实际 memory 根路径，列出目标目录并确认精确文件名；若候选列表]
[fp:bash|行动: 处理 read 报 时必读：把路径未证实视为阻断条件，先核对项目实际 memory]
[fp:grep|path not found:]

(stale: 2026-10-24 按用户要求精简项目知识：通用工具用法、单次故障 SOP 与旧阶段提示退出项目常驻记忆；操作与验证以当前工具、AGENTS.md 和 CONTRIBUTING.md 为准，原文及来源归档保留。)
