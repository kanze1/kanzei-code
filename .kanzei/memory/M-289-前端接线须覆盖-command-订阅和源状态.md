---
id: M-289
scope: project
category: fact
title: 前端接线须覆盖 command、订阅和源状态
description: 新增桌面能力、改 IPC 数据或 ESM 状态时核对真实调用链
status: active
created: 2026-10-07
updated: 2026-10-07
source: memory-manager
refs: crates/kanzei-app/src/main.rs crates/kanzei-app/src/ipc_contract.rs crates/kanzei-app/ui/01-core.js crates/kanzei-app/ui/07-events.js
subject: contract:kanzei-app:ipc-and-esm
area: kanzei-app kanzei-app/ipc_contract kanzei-app/ui/events
---

后端 command 必须进入 invoke_handler，并与真实 ESM invoke 调用的参数/返回结构一致；事件经公共 on() 订阅，listen 失败必须可见。ESM imported binding 不可直接赋值，使用拥有状态的模块导出的 setter；动态 i18n 保存源文案再 t()/rerender，不拿已翻译字符串当源。图中的目录归属不能代替 import 与事件消费者搜索；要验证真实 command→event→state→render 链。

核验方式：按 refs 读取当前实现和消费者；图用于定位，不替代源码核验。
源码审计：docs/reports/2026-10-07-boundary-knowledge.md；外部审计 episode 1335。
