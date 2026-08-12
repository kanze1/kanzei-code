---
id: M-026
scope: project
category: sop
title: test_record 请求校验缺失必补、非重试即成功可复用知识 — [fp] detection key
description: 处理 test_record 输入验证失败（缺少字段/重复提交）必读：补全必填字段再发，避免环境误判为死路
status: deprecated
created: 2026-08-09
updated: 2026-08-12
source: memory-manager
subject: cargo test input contract
---

test_record tool rejects JSON with missing required field `title`. Invalid input error: "invalid input for tool `test_record`: missing field `title`".

Error marker (fp detection key): [fp:test_record|invalid input for tool 'test_record': missing field 'title']

Action: Verify all fields present in test_record.json before invocation; do not proceed to retry until title is included.
