---
id: M-013
scope: project
category: fact
title: 处理 edit 替换失败/换行符问题：先 read 重读再改
description: 处理 edit/req 的 unknown id、替换失败、换行符不匹配，或 browser/read 报本地文件不存在/无法解析时必读：不要重复提交失效 id 或原路径；先 read 重读当前内容、目录和可用标识，核对文件是否真实存在，再按实际文本、合法 id 与已确认路径精确构造修改。
status: active
created: 2026-08-08
updated: 2026-08-28
source: inbox note 2026-08-08 [fp:bash|exit code:]
---

[fp:bash|行动:git commit 失败(exit code、"Changes not staged for commit")时必读：先检查同批前置 git add]
[fp:read|cannot open 系统找不到指定的文件。 (os error )]
[fp:read|cannot open 系统找不到指定的路径。 (os error )]
[fp:read|不能打开系统找不到指定文件.os error ]
[fp:req|unknown id ; existing: R-, R-, R-, R-, R-, R-, R-, R-, R-, R-, R-, R-, R-, R-, R]
[fp:browser|本地文件不存在或无法解析: (系统找不到指定的路径。 (os error ))]
错误原文：本地文件不存在或无法解析: crates/kanzei-app/ui/index.html (系统找不到指定的路径。 (os error 3))
处理 edit/req 报 unknown id、替换失败或换行符不匹配：不要重复提交失效 id；先 read 重读当前内容与可用标识，再按实际文本和合法 id 精确构造修改。若 browser/read 报路径不存在，先核对项目根路径、目录与目标文件确实存在，停止原路径重试后再修改。
