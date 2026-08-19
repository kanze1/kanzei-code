---
id: M-077
scope: project
category: fact
title: Windows GUI 进程(kzapp)跑 git/cargo 子进程必须隐藏控制台窗口否则弹黑窗(D-369);三处调用全查+提交门禁顺序
description: kzapp GUI 提交/跑 cargo 弹黑窗时必读:所有 git/cargo 子进程须隐藏控制台窗口(D-369),且提交门禁顺序 stage→test→record→commit
status: deprecated
created: 2026-08-16
updated: 2026-08-18
source: inbox 2026-08-14
refs: D-369 D-238
---

kzapp 是 Tauri GUI 进程无控制台,任何 std::process::Command/tokio::process::Command 跑控制台程序(git/cargo)若未设 CREATE_NO_WINDOW,Windows 会新建控制台窗口闪现。修复路径:同步命令用 crate::hide_console(std Command,kanzei-tools lib.rs:100)或 state::hidden_command(kanzei-app);tokio 命令用 hide_console_async 或 tokio Command 的 creation_flags(0x08000000,固有方法不需 CommandExt trait)。D-238 修 async 路径时漏了两处同步 git 调用(git.rs staged_source_fingerprint/staged_paths_sync)与 auto_push 的 tokio git push——排查新 git 调用时三处都要查(同步/async/auto_push)。另:test_record 的源码指纹基于暂存区,提交门禁正确顺序是 stage → cargo test → test_record → commit,先测试后 stage 会被指纹门禁反复拦截。

(auto-deprecated: candidate 超出健康水位 24，按低价值优先清退；fingerprint=false，recurrence=0；原路径 C:\Users\kanzei\Documents\kanzei code\.kanzei\memory\M-077-windows-gui-进程-kzapp-跑-git-cargo-子进程必须隐藏.md)
