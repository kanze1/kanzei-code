---
id: M-198
scope: project
category: fact
title: D-480 req unknown id 根因（R-编号规范不一致）
description: 处理D-480(fixed)任务时req命令返回unknown id R-216的根本原因分析
status: candidate
created: 2026-08-17
updated: 2026-08-17
source: memory-manager
---

当执行 req 命令搜索 R-编号时出现unknown id错误（例R-216）且现有R-列表包含其他ID（如R-286, R-283...），表明内存控制面存在ID格式规范约束或检索逻辑不匹配：①R-编号必须匹配特定模式；②系统可能要求前缀校验；③检索器未正确解析新创建资源的元数据。根本原因：工具契约层面ID格式或检索规则未被正确实现，导致后续命令链断裂
