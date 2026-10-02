---
id: M-053
scope: project
category: sop
title: M-021:defect/req update 字段中文键名避免追加脏数据 SOP — [fp:tool] detection key
description: 处理 defect/req update 导致新键被意外追加或原有整段丢失时必读：引擎按键名精确匹配，英文未知键会被追加；进展多行值会作为段落追加且不删除游离文本;update前先get读当前字段值再构造 payload。
status: deprecated
created: 2026-08-11
updated: 2026-08-12
source: memory-manager
refs: D-204 D-239
superseded_by: M-044
---

> 墓碑:2026-08-12 库存合并,本条已并入 M-044(内容重复,原文保留供追溯)。
[fp:tool|defect update field parsing bugs] 复用以下三原则避免脏数据：

1.英文键名陷阱——引擎按键名精确匹配，未知键会被追加为 new key（非覆盖）
   -现象：传{"priority": "P3", ...}后出现 `- priority: P3`新键而原有`- 优先级:P2`丢失;多轮update累积更多脏键。
   -判据：若 payload中包含英文关键字段(priority/description/progress等)，必被追加而非更新；中文键(优先级/进展/阻塞)能触发精确匹配替换机制。
   -操作：defect update 必须使用中文键名(`优先级`/`进展`/`阻塞`)构造 json，严禁用英文 key;确认 payload仅含已知键值对。

2.单行vs多行的语义差异 + 游离段落永除bug ——进展字段追加而非替换逻辑
   -现象：传递单行数 → `- 进度:`首行情字替换；传递带换行符的多行数 →作为新段落追加到文件末尾;同时产生"游离段落"(无`-键::前缀的文本行)，这些一旦生成永久残留——任何后续 update、git restore/tool直接写入都无法消除。
   -判据：payload中值含换行→多行处理模式(首行被替/其他段追加)；若需整字段替换 →必须确保 payload仅单行或单块内容;避免传递"带空白分隔符的多段文本"(如旧+新分段拼接)。正确做法:读取当前进展,将新旧内容连为一行长字符串后传。

3.清空陷阱——空值仍留空键
   -现象：传`{"进展":""}`导致产生`- 进展:`行但内容为空(解析层可能忽略，视觉上留下脏键)。
   -判据：payload中某字段为 null/string("") →引擎生成对应 key+empty-value;不能通过"清空"达到删除 effect。

完整流程 SOP:step1) run get/read target defect record,保存当前 fields 快照(step2)构造 payload→中文全 keys(优先级/进展/阻塞);单行值=新旧内容拼接后的一整串字符(如`旧进度与本次更新：... + 今日新增项`) →绝不传带换行的多行;step3)执行update;(optional验证:run get 再次确认 field content)。

边界/例外:D-204引擎层面缺乏「进展历史去重」或「字段物理删除」能力→任何清理需求都需先理解"追加机制不可逆、游离段落永残";多次重复update只会累积污染而非修复;需要回滚必须通过 git history checkout 或 diff revert。
