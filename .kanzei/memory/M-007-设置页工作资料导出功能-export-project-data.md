---
id: M-007
scope: project
category: fact
title: 设置页工作资料导出功能(export_project_data)
description: 需要了解/修改设置页导出记忆、需求、缺陷、项目配置功能(实现位置、目录约束、返回值)时必读
status: active
created: 2026-08-07
updated: 2026-08-07
source: inbox:2026-08-07
---

设置页提供可选工作资料导出,并显示实际导出路径。
- 后端:crates/kanzei-app/src/main.rs 中 export_pick_dir / export_project_data,注册在 invoke_handler。
- 前端:ui/index.html 与 ui/main.js 设置页"工作资料导出"区。
- 行为:复制 .kanzei/memory、requirements/requirements-archive、defects/defects-archive、可选 .kanzei/kanzei.toml;拒绝输出目录位于项目目录内;返回 path/files。
