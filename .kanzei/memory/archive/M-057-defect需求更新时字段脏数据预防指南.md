---
id: M-057
scope: project
category: sop
title: defect需求更新时字段脏数据预防指南
description: defect/req字段更新防重复键名与整值替换:D-204单线replace多行append字义、D-239游离段永不可清除机制
status: deprecated
created: 2026-08-12
updated: 2026-08-12
source: memory-manager
refs: D-204 D-239
---

defect/req字段更新防脏机制:a)引擎按键名字典表只认exact match:传递英文key(如priority/P3)会追加新text行("- priority:P3"原"- 优先级:P2"(中文键))，传数字id必须拼完整key否则产生双条记录。
b）progress字段的单line/- 进展:text内容替换首行、multiline/含内部分换行的input直接append到文件末尾(不原地replace)导致多批交付证据永久丢失(D-239:验收复核从2份→6重;D085第二轮复叠)。root cause D-204引擎按「是否包含控制字符」判定mode而非用户意图。
c）游离段(fluid paragraph):无"- key:"前缀的text行(仅由multiline追加产生的新段落)一旦被写入永不可删—tracker write denied、git操作被autonomous层拦截(bash shell整文件重写均跳过)、任何工具调用都失败@D-239。
d）空字符串陷阱:清空field留下原key文本如"- 优先级:"(仅清content不改结构),后续update仍遇到empty field解析歧义——需先get确认再改内容。e) CRLF转LF转换层改变行尾风格导致匹配偏差@D-245(future)。
操作规范:f)通用field:单line替换时务必确认无内部分换行;g）progress字段特殊处理：single-line content=replace首行、multiline input(哪怕仅含一个\n)=append到末尾→get后手动拼接旧text+new内容为完整string，绝不传raw multiline param触发系统级append。h)空字符串清空field前先确认是否要删key本身——解析层ignore empty value不delete key文本。i）历史堆叠：每轮成功update只改首行→游离段累积(newline插入位置+旧数据叠加=不可逆膨胀)。
关键陷阱:j) D-204引擎无字段结构aware，跨平台Windows/Mac/Linux行为一致(knowing engine behavior≠copy-pase fix)
k）D-239缺失「进展历史去重/场段删除工具」需依赖外部git restore但会破坏tracker integrity@M-318。l)fp标记:两次复发(注意edit与field update不同机制仍指向同一根因—用户误传multiline param或空字符串逻辑混淆)。
[fp:bash|git restore is blocked in bash: Git mutations must use the structured git to] [fp:edit|这次替换净删除行(行换成行)]
