---
id: M-280
scope: project
category: fact
title: CLI 是共享能力的命令行入口
description: 改 kz run、根解析、历史恢复或 CLI 收尾时核对共享能力入口
status: active
created: 2026-10-07
updated: 2026-10-07
source: memory-manager
refs: crates/kanzei/src/main.rs crates/kanzei/src/cli/run.rs crates/kanzei/src/cli/run/finalize.rs
subject: responsibility:kanzei
area: kanzei
---

kz CLI 解析命令、主项目根、真实输入身份、权限应答和运行收尾，使用 tools/run 公共装配与 core run_once。CLI 与桌面主线共享 project_session_id，--new 是新段，不是另造独立历史库。较长输入通过 --prompt-file；入口不能另实现记忆检索、装配或 session purge 规则。CLI 收尾调用公共 memory_consolidation。

核验方式：按 refs 读取当前实现和消费者；图用于定位，不替代源码核验。
源码审计：docs/reports/2026-10-07-boundary-knowledge.md；外部审计 episode 1335。
