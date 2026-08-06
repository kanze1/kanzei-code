# kanzei

自用 AI coding agent,Rust 重写自 [opencode](https://github.com/anomalyco/opencode)(参考其 V2 架构),核心卖点是自研 **harness 系统**。

设计原则:**好用压倒一切**(自用工具,按自己的使用习惯优化)。四大关注点:harness 设计、内存占用、记忆管理、loop 调度。

## 结构

| Crate | 职责 |
|---|---|
| `kanzei-harness` | 统一扩展层:六注册表(agents/tools/commands/skills/context-sources/permissions)+ 拦截器链硬门禁 + 双模式 profile(dev/research) |
| `kanzei-llm` | LLM 协议层:Anthropic Messages + OpenAI Chat 兼容(Ollama/LM Studio/DeepSeek/Kimi...),统一 LlmEvent 流,代理支持(loopback 豁免) |
| `kanzei-core` | session 运行时:runner 循环、调度(steer/queue)、SQLite 事件溯源、压缩 |
| `kanzei-tools` | 内置工具:read(反向 seek tail)/ write(写后校验)/ bash(动态 shell 检测)... |
| `kanzei` | CLI 入口(`kz`);M3 起为 Tauri 桌面端的后端 |

## 安装

**安装包(推荐)**:从 [Releases](https://github.com/kanze1/kanzei-code/releases/latest) 下载 `kanzei-setup-*.exe` 双击安装(用户级,含开始菜单/卸载器)。应用启动会静默检查新版本,设置页可一键更新。

**源码构建(开发机)**:

```powershell
.\scripts\release.ps1            # test → release build → 安装 kz + kzapp 到 ~/.cargo/bin(带 pending 自更新)
.\scripts\package.ps1 -Publish   # 打 NSIS 安装包并发布到 GitHub Releases(安装版的更新源)
kz --version
```

## 使用

**桌面端(主要入口)**:`kzapp` — 类 VSCode 布局:项目管理 / 需求缺陷侧边栏(可展开、状态流转)/ 流式对话(工具块折叠)/ 模型直选 / 权限弹窗(拒绝/一次/总是)/ 运行日志面板 / 设置页。

**CLI**:`kz run "任务"`,`kz req|defect|source|finding list|add|...`(人用直通)。

**模型通道**(`~/.kanzei/kanzei.toml`,设置页可视化编辑 + 一键测试):
- `codex:gpt-5.6-sol|terra|luna` — 复用 Codex CLI 订阅登录态,自动刷新
- `claude:claude-opus-5|sonnet-5|haiku-4-5` — 复用 Claude Code 长效令牌(`claude setup-token`)
- `kimi:kimi-k3` 等任意 OpenAI 兼容端点(key 支持环境变量或直填)
- `ollama:<model>` — 本地模型,loopback 自动免代理;fast 角色驱动并行 task 子代理

**权限**:Ruleset last-match-wins,默认 ask;弹窗选"总是允许"会把泛化规则(bash 取命令首词前缀)写进项目 `.kanzei/kanzei.toml`。

## 进展

- ✅ M0-M2:三协议 LLM 层、harness 六注册表与双模式、SQLite 事件溯源、steer/queue 调度、会话持久化与回放、自动压缩
- ✅ 多 agent:task 并行子代理(fast/primary 双档)、双状态人格(结伴开发/自主推进)、连跑与目标驱动自举
- ✅ 桌面端:对话为主布局 + 右侧活动面板、历史对话管理、附件(图片/PDF)、todo/question 工具、发行版安装包与应用内更新
- ⏳ 进行中:多进程解耦(R-030)、并行对话线程与 worktree 合并(R-050)、MCP

日常开发由 kanzei 自举完成(dev agent 按 backlog 连跑),需求/缺陷全录在 `.kanzei/project/`。

设计规格参照:`docs/design/harness-m1.md` 与 `docs/reference/`(拷自 opencode 的 CONTEXT.md 与 specs/v2)。
