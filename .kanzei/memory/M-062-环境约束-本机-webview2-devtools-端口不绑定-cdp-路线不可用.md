---
id: M-062
scope: project
category: fact
title: 环境约束:本机 WebView2 151 DevTools 端口从不绑定,e2e CDP 路线不可用
description: 想走 e2e-smoke / connectOverCDP / WebView2 DevTools 端口路线前必读:当前机器已 9 轮实验证实不可用,不要重推
status: active
created: 2026-08-13
updated: 2026-08-13
source: inbox 升格(2026-08-13;原 inbox 条目随清理丢失,内容自探查代理摘录恢复)
---

本机 WebView2 Runtime 151 在任何注入通道下都不启动 DevTools HTTP 服务:e2e-smoke 的 connectOverCDP 20 秒超时,参数已传入但端口不绑定。D-319 的 9 轮实验已逐一排除注入通道、参数格式、注册表策略、AppContainer、--enable-logging 日志、WEBVIEW2_FIXED_VERSION、进程树七类假设(有 Edge 对照组),结论限定在"当前机器"。

**不要重推 WebView2 CDP 路线**。D-319 已登记阻塞,解除动作只有三条互斥路径:用户重装/更新 WebView2 Runtime、换环境、或等 runtime 修复——解除人是用户。D-289 的 CDP 参数补齐是正交的正确修复,不因环境阻断而回滚。

refs: D-319 D-289
