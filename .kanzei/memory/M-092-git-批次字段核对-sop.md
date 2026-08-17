---
id: M-092
scope: project
category: sop
title: Git 批次字段核对 SOP
description: 处理发版/发布/装机时必读:核对 Git 提交历史与手写批次数是否一致;已确认第 3 次复发并有修复验证
status: active
created: 2026-08-17
updated: 2026-08-17
source: user:note-2026-08-13
---

指纹：[fp:req|R- 的手写批次是 Git 提交历史标记数为 ;请先核对并更新批次字段后再关闭。], 原样放进正文——它是复发检测的键，丢了引擎就看不见「记了但没用」。

判断要点：Git 提交历史与手写批次不匹配是环境工具契约错误(非一次性噪声)。
