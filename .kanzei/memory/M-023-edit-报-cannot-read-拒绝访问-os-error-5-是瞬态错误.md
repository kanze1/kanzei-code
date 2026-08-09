---
id: M-023
scope: project
category: fact
title: edit 报 cannot read 拒绝访问 (os error 5) 是瞬态错误,重试即成功
description: 处理 edit 报 "cannot read ... 拒绝访问 (os error 5)" 时必读:这是 Windows 瞬态访问拒绝,不是真实权限/路径问题——先 read 重读再重试 edit 即可成功,不要改 bash 绕过,也不要误判为死路而放弃。
status: active
created: 2026-08-09
updated: 2026-08-09
source: inbox 2026-08-09
---

错误原文: cannot read C:\Users\kanzei\Documents\kanzei code\crates/kanzei-app: 拒绝访问。 (os error 5)。[fp:edit|cannot read 拒绝访问。 (os error )]
出现于 edit 操作,重试 1 次即成功。判据:os error 5 (拒绝访问) 在 edit 读取阶段是瞬态错误,不代表文件被占用或权限真实缺失;遇到时先 read 重读确认文件内容,再重试 edit,不要 panic、不要换 bash 绕过。
