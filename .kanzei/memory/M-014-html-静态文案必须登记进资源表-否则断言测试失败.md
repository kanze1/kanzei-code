---
id: M-014
scope: project
category: fact
title: HTML 静态文案必须登记进资源表,否则断言测试失败
description: node 断言测试报 "HTML 静态文案未进入资源表"(AssertionError,exit code 1)时必读:新增/修改 HTML 静态 UI 文案后必须同步登记到资源表
status: active
created: 2026-08-08
updated: 2026-08-08
source: memory-manager
---

项目内存在校验(通过 node 运行,exit code 1):`AssertionError [ERR_ASSERTION]: HTML 静态文案未进入资源表: 切换到对话 | 切换到工作区 | 切换到需求与工作和缺陷 | 切换到记忆 | 运行画像 | 切换到运行画像 | 切换到设置 | 初始化新项目目录 | 添加项目目录 | 删除勾选的对话 …`。

契约:ui/index.html 等 HTML 中的所有静态 UI 文案都必须登记进 i18n 资源表;新增/修改 HTML 静态文案后必须同步更新资源表,否则该断言测试失败。这是环境契约,不是测试本身有 bug——修复方向是把缺失文案补进资源表,而非绕过或重试。与 M-001(动态 i18n 需保存源文案)互补:那条管动态 textContent,这条管 HTML 静态文案的资源表登记。
