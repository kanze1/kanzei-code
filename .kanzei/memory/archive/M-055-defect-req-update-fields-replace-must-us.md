---
id: M-055
scope: project
category: sop
title: defect/req update fields replace must use Chinese keys, English creates duplicates — [fp:edit|field replacement needs]
description: 处理 defect/req update 字段替换失败 (整段替代+中文键名必须)时必读:英文 key(name priority)会被当作新密钥追加致脏数据,单字串值会覆盖首行但多行会变成游离段落永远清除不掉 -- 更新前先读当前内容;用优先级进展阻塞等中文名
status: deprecated
created: 2026-08-12
updated: 2026-08-12
source: memory-manager
refs: D-204
subject: defect/req update fields
---

2026-08-13 实测:defect update D-204传{"priority":"","进展":"..."}后，文件里出现-new priority:P3新键(原有-new优先级:P2未被更新),且字段内容被整段替换——原两轮交付证据丢失,需从git diff找回再update恢复。正确做法:①update前get读当前所有值;进展多行必须拼上旧内容+新内容的整体传;②关键用「优先级」「进展」等中文名键，引擎按键名精确匹配更新,英文key是未知key会被追加;③清空field留空字符串会留下空key(解析层忽略不删)，不要指望会自动移除
