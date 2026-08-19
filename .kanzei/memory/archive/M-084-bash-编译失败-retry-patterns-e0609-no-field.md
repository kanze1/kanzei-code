---
id: M-084
scope: project
category: fact
title: bash 编译失败 retry patterns (E0609/no field/missing field)
description: 处理 bash/编译错误时必读:区分 Rust 特性错误类型，第 3 次失败带修复证据后入记忆
status: deprecated
created: 2026-08-17
updated: 2026-08-18
source: memory-manager
---

Rust 编译错误按模式分类：

- missing field in initializer: SubagentRuntime 缺 background_notifications，检查 struct 定义是否完整
- no field on type Result: `status` 字段不存在于 Result<Output, std::io::Error>,需要检查 Output 结构体定义
- the type does not implement Copy: Arc/TraitsCopy冲突，Rust要求明确类型实现特性

重复规则：第1次失败→观察记录；第2次才建candidate；第3次+带修复成功→add+promote。指纹标记[fps:bash|error code...]必须保留作为复发检测键。

(auto-deprecated: candidate 超出健康水位 24，按低价值优先清退；fingerprint=false，recurrence=0；原路径 C:\Users\kanzei\Documents\kanzei code\.kanzei\memory\M-084-bash-编译失败-retry-patterns-e0609-no-field.md)
