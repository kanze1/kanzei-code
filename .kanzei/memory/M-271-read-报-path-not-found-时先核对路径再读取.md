---
id: M-271
scope: project
category: sop
title: read 报 path not found 时先核对项目 memory 根路径与目标文件
description: 处理 read 或 grep 报 `path not found`、候选目录缺少目标文件或同类错误复发时必读：停止原路径重试，先核对项目实际 memory 根路径并用目录列表/glob 找到真实文件；未获路径证据前不得继续调用，找到多个候选先选定唯一目标。
status: active
created: 2026-08-21
updated: 2026-09-07
source: memory-manager
---

适用场景：read 或 grep 返回 `path not found`，尤其候选目录不含目标文件，或同类路径错误再次出现。
操作步骤：1. 停止沿原路径重复工具调用；判断依据是错误已明确说明目标路径不存在。2. 核对项目实际 memory 根路径并列出目标目录；以当前目录清单而非旧记忆判断路径。3. 只有目录证据确认精确文件名和完整路径后才重新 read/grep；否则改用已确认的邻近文件、glob 定位或重新定位路径。4. 多个候选时先按项目上下文选定唯一目标。
边界与例外：邻近文件只能作为定位线索，不能冒充目标文件；glob 证明的是路径而非内容匹配；文件名、扩展名或大小写未确认时不得重试。
复发检测标记：[fp:read|path not found:]
复发检测标记：[fp:grep|path not found:]
保留既有复发标记：[fp:bash|行动: 处理 read 报 path not found 或目标文件未找到时必读：先核对项目实际 memory 根路径，列出目标目录并确认精确文件名；若候选列表]
保留既有复发标记：[fp:bash|行动: 处理 read 报 时必读：把路径未证实视为阻断条件，先核对项目实际 memory]
本轮错误原文：`path not found: \\?\C:\Users\kanzei\Documents\kanzei code\crates\kanzei-tools\src\research_tracker.rs`；同目录候选包括 `research_runner.rs`、`research_write.rs` 等。
本轮 grep 证据：对 `\\?\C:\Users\kanzei\Documents\kanzei code\crates\kanzei-tools\src\docstore.rs` 报 `path not found`，改用 glob 成功。
