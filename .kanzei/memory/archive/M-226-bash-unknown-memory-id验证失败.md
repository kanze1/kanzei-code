---
id: M-226
scope: project
category: fact
title: bash unknown memory id验证失败
description: 处理bash unknown memory id错误时必读：确保memory ID存在且格式正确，避免M-前缀后跟不标准格式的ID
status: deprecated
created: 2026-08-17
updated: 2026-08-20
source: memory-manager
superseded_by: M-222
---

bash工具报错M-160 unknown memory id(2次失败后重试成功)[fp:bash|M-: ERROR unknown memory id]。
exit code:1, stderr: M-160: ERROR unknown memory id `M-160`。

root cause: bash工具在处理记忆ID引用时存在验证逻辑问题,当遇到无效或格式不正确的memory ID(如M-后跟非标准id格式)时会报错;需先确认memory ID存在且格式正确后再执行bash操作。
