---
id: M-047
scope: project
category: sop
title: defect/update-字段防脏数据守则：整键替换中文必须英文追加 -get read current fields, append single-line progress content to old value;Chinese keys only for replacement，English triggers new-field addition
description: 处理 defect update 防脏数据时必读：整键替换、英语视为追加 -get read current fields, append single-line progress content to old value;Chinese keys only for replacement，English triggers new-field addition
status: deprecated
created: 2026-08-11
updated: 2026-08-12
source: memory-manager
superseded_by: M-044
---

> 墓碑:2026-08-12 库存合并,本条已并入 M-044(内容重复,原文保留供追溯)。
实测：更新字段时，英文键名会当新键追加造成脏数据①defect update 传{"priority":"P3","进展":"..."}后文件里出现-new priority:P3(原有 - 优先级,P2 未被)；②-进展被整段替换原两批交付证据丢失。正确做法：更新前先 get read 当前字段，进展必须拼上旧内容+新内容整体传；键名用「优先㱢*/进展*阻塞」等中文，英文视为未知键追加；清空字段传空 string 会留空 key(解析层忽略不删)。
