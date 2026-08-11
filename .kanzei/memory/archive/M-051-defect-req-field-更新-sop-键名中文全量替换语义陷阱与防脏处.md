---
id: M-051
scope: project
category: sop
title: defect/req field 更新 SOP:键名中文全量替换语义陷阱与防脏处理
description: 处理 defect/req update 时必读：update fields 为整段全量更换非逐场更新，英文 key[priority]会被当新 key 追加导致重复字段；keyname 必须用「优先级/进展/阻塞」等中文键名。单行值替换对应行、多行值作为新区段落追加末尾(不覆盖原文)；游离文本段一旦生成永久留存、无工具可删，反复 update只会更脏
status: deprecated
created: 2026-08-11
updated: 2026-08-12
source: user:2026-08-11 inbox consolidation [sop]notes+D-204/D-239实测验证记录
refs: D-204 D-239
superseded_by: M-044
---

> 墓碑:2026-08-12 库存合并,本条已并入 M-044(内容重复,原文保留供追溯)。
适用场景:defect/update req 字段变更时遇到重复键、多行值膨胀或游离段落累积问题。

操作步骤:
1) pre-update get读：先读取当前目标文件，获取待更新 field所在整行或多行内容作为 baseline
2) keyname约束校验：所有field key必须使用中文(优先级/进展/阻塞)，严禁英文(key=priority将被当新key追加而非覆盖原有键的值)；判断依据：引擎按键名精确匹配更新逻辑+对未知key的append策略，详见D-204实测验证。

3) value处理规则:
   - 单行值=替换对应单行(原多行数中的首行被整行移除)→必须传single-line string;否则 multi-lines会被当新段落追加而非覆盖原文；判断依据：get读后比较旧/新版本结构差异，确保不丢失历史证据。
   
4) 防"游离段"(无`-键:`前缀的文本行):一旦生成永久留存，无任何工具(直接write/git restore/shell重写)可删除→绝不为清理重复反复update,越修越脏；判断依据：engine对file write有完整性门禁+多轮实测D-239验证。

5) 异常处理:清空字段传空字符串会留下空键;解析层忽略该key但不删除结构(导致后续更新仍可被追加到该位置);如需移除对应行必须整体re-parse file而非靠单条update实现.
