---
id: M-095
scope: project
category: fact
title: edit 替换后发现新文本行数异常多且旧内容缺失：判断为附近追加被匹配顶掉 — 处理方法是把要插入的行原样写入new_string，确需清空对应段再置allow_deletion=true
description: 处理edit发现替换后新文本行数异常多且旧内容缺失时必读:判断为意图是附近追加被匹配顶掉，此时把要插入的行原样写入new_string，确需清空对应段再置allow_deletion=true
status: candidate
created: 2026-08-17
updated: 2026-08-17
source: memory-manager
---

[fp:edit|这次替换看着像插入(新文本多了行),却没保住old_string里的原文—十有八九是想在附近加内容,结果把匹配到的那段顶掉了。要插入就把下面这些行原样写] 当editor报"new String has many more lines than expected but old string missing"或替换后文件行数异常增加时:判断为新文本追加被匹配错误覆盖;先将要写入的行原样存入new_string，然后若确需替换该段再置allow_deletion=true执行替换
