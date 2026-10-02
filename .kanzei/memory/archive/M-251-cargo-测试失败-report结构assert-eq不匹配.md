---
id: M-251
scope: project
category: fact
title: cargo 测试失败：report结构assert eq不匹配
description: 处理 test assert_eq: verify report structure consistency before retry
status: deprecated
created: 2026-08-18
updated: 2026-08-20
source: memory-manager
superseded_by: M-258
---

Fingerprint [fp:bash|assert_eq!(report.deprecated, low_value_ids);] marks first test failure in memory store assertions. Requires verifying report structure matches expected values before retry; specific assertion failures indicate data inconsistency needs source verification, not immediate compilation fix. Follows 3-retry candidate building pattern.
