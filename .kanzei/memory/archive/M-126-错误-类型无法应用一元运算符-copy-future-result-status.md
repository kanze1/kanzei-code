---
id: M-126
scope: project
category: fact
title: 错误：类型无法应用一元运算符（Copy, Future, Result status）
description: bash 编译错误类型不实现 Copy（Arc/Future） — cargo build/测试失败，修改实现后重试成功
status: deprecated
created: 2026-08-17
updated: 2026-08-18
source: memory-manager
subject: cargo build compile error type not implement Copy Future
---

错误原文: exit code 1 > error[E0382]: the type `Arc` does not implement `Copy`. -> crates\kanzei\tests\...\n + error[E0609]: no field `status` on type `Result<Output, std::io::Error>`。-> crates\kanzei-tools\src\git.rs:838 
类型未实现 Copy/Future 导致编译失败，修正实现后重试成功。[fp:bash|> error[E]: the type does not implement Copy / no field on type Result]

(auto-deprecated: candidate 超出健康水位 24，按低价值优先清退；fingerprint=true，recurrence=0；原路径 C:\Users\kanzei\Documents\kanzei code\.kanzei\memory\M-126-错误-类型无法应用一元运算符-copy-future-result-status.md)
