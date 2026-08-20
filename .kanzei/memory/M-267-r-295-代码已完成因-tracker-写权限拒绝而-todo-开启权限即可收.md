---
id: M-267
scope: project
category: fact
title: R-295 代码已完成因 tracker 写权限拒绝而 todo,开启权限即可收口
description: 处理 R-295 健康水位/低价值清退任务时必读:代码与验收已全部完成,不要再做开发或重跑验证——先检查本线 tracker 写权限是否已开启;已开启则直接 req update R-295 done(进展字段已备好逐条验收文本),未开启则先启用写权限,切勿当作未完成返工。
status: active
created: 2026-08-20
updated: 2026-08-20
source: run:episode-898
refs: R-295
subject: R-295 收口状态
---

R-295 状态:代码与验收已全部完成,唯一卡点是本线 tracker 写权限被拒(req update / req claim 均 permission denied),条目仍为 todo。\n完成证据:提交 9c5a89ea(B1 健康水位 CANDIDATE_MAX_COUNT=24 + 低价值优先清退 + 测试)、150c6cdb(B2 untouched 语义修正);全量 cargo test --workspace 全绿(T-1786922726367:1214 passed 0 failed);真实存量处置 candidate 153→24(129 条 deprecated 入归档带墓碑,临时 example reconcile_r295 执行后已删除);检索窗口 top-24 candidate 占用 bash 21→14、记忆 20→9、cargo 23→8;验收证据在 T-1786922726360/6364/6365/6367/6368/6369。\n剩余动作:开启本线 tracker 写权限后执行 req update R-295 done;进展字段已备好逐条验收文本,无需重写。\n关联 [fp:work|permission denied by ruleset: work on .]:work on write:claim 被 ruleset 拒绝——当前分支线未开启 tracker 写入,读取仍可用;在该线设置中显式开启后再修改唯一主根文档。
