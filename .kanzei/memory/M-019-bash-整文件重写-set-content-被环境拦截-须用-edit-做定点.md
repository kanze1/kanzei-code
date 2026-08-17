---
id: M-019
scope: project
category: sop
title: bash 整文件重写(Set-Content)被环境拦截,须用 edit 做定点修改
description: 处理 bash 整文件重写(Set-Content/Out-File/full-file-write guard)被环境拦截时必读:改用 edit 定点修改,勿试探 shell 整写;.kanzei 下 policy-managed 文件只能用专用工具。
status: active
created: 2026-08-08
updated: 2026-08-16
source: fp:bash 拦截, 2026-08-08
---

[fp:bash|`set-content` is blocked: whole-file rewrites via shell bypass the tools' syntax validation and diff display] [fp:bash|`out-file` is blocked: whole-file rewrites via shell bypass the tools' syntax validation and diff display] [fp:bash|is blocked: whole-file rewrites via shell bypass the tools' syntax validation and diff display]
bash 整文件重写(Set-Content/Out-File/重定向/remove-item 重建)被环境拦截,报 "whole-file rewrites via shell bypass the edit/write tools' syntax validation and diff display"/"permission denied by guard `full-file-write`" 时必读:整文件写被环境拦,改用 edit 做定点修改(它容忍行尾差异、连失两次后显示文件实际内容);不要试探别的手段绕。.kanzei/project、.kanzei/memory 下 policy-managed 文件无论怎么 shell 写都被回滚,唯一合法通道是 req/defect/test_record 等专用工具(M-005)。
复发判据(2026-08-13~16 多文件复发):任何想用 shell 重写文件内容的意图都应直接走 edit/专用工具,勿再试 shell 整写。
