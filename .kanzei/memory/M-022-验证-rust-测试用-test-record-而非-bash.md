---
id: M-022
scope: project
category: sop
title: 验证 Rust 测试用 test_record 而非 bash — 任何涉及 cargo test 的动作第一步必读
description: 任何涉及 cargo test / run test / 验证 Rust 测试的动作第一步必读——即使已有本条记忆、即使只差一步验证,验证通道唯一是 test_record,bash 不在候选集(不是"优先",是"唯一");若已误开 bash 跑 cargo test 且 stderr 报 exit code: 1 或任何编译警告/error,立即停手换 test_record,不要继续在 bash 里排查或为警告改代码。
status: active
created: 2026-08-09
updated: 2026-08-09
source: inbox:2026-08-09
---

[fp:bash|exit code:]
验证 Rust 测试的唯一通道是 test_record,bash 不在候选集(不是"优先",是"唯一")。判据不依赖认出具体警告文本:只要动作涉及 cargo test / run test / 验证 Rust 测试,第一步直接选 test_record;若已误开 bash 跑 cargo test 且报 exit code: 1、stderr 出现任何编译警告/error(本轮 2026-08-09 复发:crates\kanzei-core\src\runner.rs:1492:26 "warning: value assigned to `final_text` is never read",与此前同类),立即停手换 test_record,不要继续在 bash 里排查或为警告改代码。本坑已多次复发(即使已有本条记忆仍复发),动手前先自问"我要验证 Rust 测试吗",是则第一步直接选 test_record。
