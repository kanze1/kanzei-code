---
id: M-058
scope: project
category: sop
title: 发版 SOP:dev 全绿 → 发布树 ff → verify 十步 → package -Ack → 容器外静默装
description: 执行发版动作(打包/发布/装机)时必读:标准步骤序列与三个已知坑位
status: active
created: 2026-08-13
updated: 2026-08-13
source: 会话 2026-08-13 发版实操复盘(build-fe26bb7)
---

标准序列(验证通过即执行,无需逐步征求同意):

1. **dev 树收干净**:改动全部提交(源码提交须有新鲜 passed 测试记录背书),测试记录必须与引用它的证据同批入库(防归档指针悬空,实例:T-1786574944),push origin dev。
2. **发布树同步**:发布树 `C:/Users/kanzei/Documents/kanzei-release`(checkout main)内 `git fetch origin` → `git merge --ff-only origin/dev` → `git push origin main`。**绝不手动往发布树同步文件**——同内容一旦在 dev 提交,手动副本会堵死快进;误堵后用 `git show HEAD:<f>` 按 CRLF 形态写回 + `git update-index --refresh` 恢复(autocrlf 下 merge 只看 stat 缓存)。
3. **十步门禁**:发布树跑 `scripts/verify.ps1`,全绿产出绑定 HEAD commit 的 `dist/verification.json`(工作树必须干净,含未跟踪源码检查)。
4. **打包发布**:`scripts/package.ps1 -Ack <N> -Publish`。N = 自上个 build-* 标签以来的提交数,**先逐条核对清单再填**(D-183 防夹带门禁,数目不符即停,多出来的可能是并发运行的提交)。产出 `dist/kanzei-setup-<hash>.exe` 并发 GitHub Release,应用内"检查更新"以此为源。
5. **装机在 Claude 容器外**(LOCALAPPDATA 会被容器重定向到影子目录):`scripts/install-setup.ps1 -Setup dist\kanzei-setup-<hash>.exe -ExpectedHash <hash>`。kzapp 运行中静默安装会"退出码 0 但没装上",脚本信产物不信退出码——装前先退出 kzapp。

相关:M-005(托管文件走专用工具)、M-029(git mutation 走结构化工具)。
