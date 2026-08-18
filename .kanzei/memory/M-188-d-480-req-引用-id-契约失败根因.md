---
id: M-188
scope: project
category: fact
title: D-480(req)引用 ID 契约失败根因
description: 处理 req 步骤 unknown id 错误/引用格式契约失败
status: candidate
created: 2026-08-17
updated: 2026-08-17
source: memory-manager
---

根因模式：工具调用要求严格匹配的引用 ID 格式。当 req 步骤出现 unknown id `R-216`且预期存在一系列 R-xxx 引用时，表示实际传入的 ID 与系统预期的 R-286/283/284/285/287/235/101 等基准 ID 不匹配。这属于工具契约错误：req 步骤依赖的引用对象必须预先存在于系统中且格式一致，否则触发 unknown id 异常。修复策略：核对引用来源 R-xxx,确保其完整存在并符合预期标识规范后再执行后续流。
