---
id: M-202
scope: project
category: fact
title: bash 环境契约失败时改用 test_record 记录验证
description: 处理 bash/cargo 验证因运行环境契约失败、尤其报 not a git repository 而 test_record 可成功记录时必读：不要继续重试 bash；改用 test_record，并保留完整错误文本判断是否为环境问题。
status: deprecated
created: 2026-08-17
updated: 2026-10-07
source: user
subject: bash超时失败
area: kanzei-tools/bash kanzei-tools/test_record
---

可复用环境/工具契约：bash 执行 cargo 验证时若工作目录不是 Git 仓库，出现“fatal: not a git repository (or any of the parent directories): .git”并以 exit code 1 失败，不要把测试输出中局部的 ok 当作成功，也不要反复重跑 bash；改用 test_record 记录验证结果。已有测试成功输出只能作为证据的一部分，仍须保留完整失败原因。

[fp:bash|fatal: not a git repository (or any of the parent directories): .git]
证据：本轮 cargo 运行约 169 tests，部分测试显示 ok，但 bash 仍 exit code 1；改用 test_record 成功。

(stale: 2026-10-07 用户授权项目记忆清理；源码复核：test_record 登记证据不会修复非 Git 工作目录或失败的执行；记录成功不能代替重新执行与退出状态核验，保留 M-258 的完整输出核对原则。)
