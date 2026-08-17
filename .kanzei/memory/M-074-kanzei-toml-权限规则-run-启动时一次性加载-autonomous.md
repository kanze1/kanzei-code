---
id: M-074
scope: project
category: fact
title: kanzei.toml 权限规则 run 启动时一次性加载,autonomous 会话内加白名单不生效,需下个 run 重载
description: autonomous 会话改 kanzei.toml 加白名单当轮 work claim 仍被拒时必读:权限规则 run 启动时一次性加载,下个 run 才生效
status: candidate
created: 2026-08-16
updated: 2026-08-16
source: inbox 2026-08-14
refs: D-209
---

kanzei-app/src/run.rs:107 KanzeiConfig::load_with_warnings_at_root 在 run 启动时加载;harness.rs:253 ConfigComponent::contribute 把 config.permissions.rules 拷贝进 draft.permissions,之后 evaluate 走内存 draft 不重读文件;drive.rs:1181-1186 在 AskPolicy 不允许用户提示时把 Ask 短路为 Gate::NonInteractive。因此 autonomous 会话中通过 edit 修改 .kanzei/kanzei.toml 添加权限白名单,当轮 work claim 仍会被拒;正确预期是提交后下个 run 生效。work claim 的权限键格式:action="work"、resource="write:claim"(work.rs resources() 对 claim 用 "write:{action}" 前缀)。
