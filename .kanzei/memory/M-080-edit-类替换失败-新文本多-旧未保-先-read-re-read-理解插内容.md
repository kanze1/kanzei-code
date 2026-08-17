---
id: M-080
scope: project
category: sop
title: edit 类替换失败(新文本多/旧未保)：先 read re-read 理解插内容意图再执行
description: 编辑替换失败：新文本多行但旧字符串未保留——须重读再构造，勿盲目 replace_all
status: candidate
created: 2026-08-17
updated: 2026-08-17
source: memory-manager
---

编辑替换失败且新文本多了多行、旧字符串未被保留时：十有八九是想在附近加内容，但匹配到的那段被顶掉了。须先 read 重读文件理解上下文，将需插入的行原样写进 new_string；确实要替换掉它们，才置 allow_deletion=true。关键判据：观察替换后老文本是否还在——若不在了且多了行，说明未正确处理替换意图。保留标记：[fp:edit|这次替换看着像插入(新文本多了 10 行),却没保住 old_string 里的原文——十有八九是想在附近加内容,结果把匹配到的那段顶掉了。要插入就把下面这些行原样写]
