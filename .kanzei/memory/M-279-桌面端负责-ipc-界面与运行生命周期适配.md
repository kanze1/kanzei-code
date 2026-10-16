---
id: M-279
scope: project
category: fact
title: 桌面端负责 IPC、界面与运行生命周期适配
description: 改桌面 command、事件、会话切换或页面时核对前后端接线
status: active
created: 2026-10-07
updated: 2026-10-07
source: memory-manager
refs: crates/kanzei-app/src/main.rs crates/kanzei-app/src/run/coordinator.rs crates/kanzei-app/ui/07-events.js
subject: responsibility:kanzei-app
area: kanzei-app
---

kanzei-app 将 Tauri command、项目/过程身份、排队与运行协调、桌面资源和 ESM UI 接到共享运行能力。invoke_handler 是 command 注册入口；07-events.js 等订阅运行事件。新增命令必须核对注册、真实调用者、入参与返回结构；事件须通过公共 on() 订阅并显示失败。界面数组不是 session/process/typed facts 的持久真源，不能靠删侧栏项完成生命周期清理。

核验方式：按 refs 读取当前实现和消费者；图用于定位，不替代源码核验。
源码审计：docs/reports/2026-10-07-boundary-knowledge.md；外部审计 episode 1335。
