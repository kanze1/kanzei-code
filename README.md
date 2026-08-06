# kanzei

**中文** | [English](#english)

[![release](https://img.shields.io/github/v/release/kanze1/kanzei-code?label=release&color=c9962e)](https://github.com/kanze1/kanzei-code/releases/latest)
![platform](https://img.shields.io/badge/platform-Windows-2ea44f)
![built with](https://img.shields.io/badge/Rust%20%2B%20Tauri-19MB%20installer-8fae8a)

> 一个**自己开发自己**的 AI coding agent:桌面端里的 agent 按 backlog 连跑,把 kanzei 的下一个功能写出来、测完、提交、发版。你现在看到的大部分代码,就是它自己写的。

自用 AI coding agent,Rust 重写自 [opencode](https://github.com/anomalyco/opencode)(参考其 V2 架构),核心是自研 **harness 系统**。设计原则:**好用压倒一切**。

## 和 Claude Code / Codex CLI 有什么不同

kanzei 深受它们启发,但走了几条它们没走的路:

| | kanzei 的做法 |
|---|---|
| **规则在代码,不在提示词** | 权限、文档格式、状态机、引用完整性全是**硬门禁**(注册表 + 拦截器):模型格式写错会被引擎拒绝并教它重试,而不是靠 system prompt 恳求 |
| **工单即文件** | 需求/缺陷/目标是纯 markdown,你手改、agent 用工具改,同一份文件;拖拽排序 = 改文件顺序,agent 按你排的顺序取活 |
| **订阅复用 + 多通道混跑** | Codex 订阅、Claude Code 订阅、Kimi API、本地 Ollama 在同一界面切换;并行子代理默认跑本地小模型,探索零成本 |
| **透明压倒一切** | 上下文占用进度条、压缩可召回、子代理实时轨迹、权限规则可查——agent 看到什么、花了什么,你都看得到 |
| **双人格** | 结伴开发(对话优先,拿不准就问)与自主推进(backlog 驱动,连跑不问)是两个 agent,不再互相打架 |
| **轻** | Rust + Tauri,安装包 5MB、二进制 19MB,无 Node 运行时 |
| **为一个人服务** | 没有企业功能、没有多租户:交互按主人的习惯生长,这是它敢叫"好用压倒一切"的原因 |

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

---

## English

> An AI coding agent that **develops itself**: the agent inside the desktop app works through its own backlog — writing, testing, committing, and releasing kanzei's next feature. Most of the code you're reading was written by it.

Personal AI coding agent, rewritten in Rust from [opencode](https://github.com/anomalyco/opencode) (modeled on its V2 architecture). The core differentiator is a custom **harness system**: six registries (agents / tools / commands / skills / context sources / permissions) resolved into immutable snapshots, with hard gates enforced in code rather than prompts.

**Design principle: usability above all** — this is a personal daily-driver tool, optimized for transparent context management, multi-agent collaboration, speed, minimal interruptions, and clear information.

### How it differs from Claude Code / Codex CLI

Deeply inspired by both — but it takes a few roads they don't:

- **Rules live in code, not prompts** — permissions, doc formats, state machines, and citation integrity are hard gates (registries + interceptors); a malformed tool call gets rejected with a repair hint instead of being begged away in the system prompt
- **Tickets are files** — requirements/defects/goals are plain markdown that you and the agent co-edit through the same engine; drag-to-reorder the list and the agent works top-down in *your* order
- **Subscription reuse, mixed fleets** — Codex login, Claude Code token, Kimi API, and local Ollama side by side; parallel subagents default to the free local model
- **Transparency above all** — context meter, recallable compaction, live subagent traces, inspectable permission rules: you always see what the agent sees and spends
- **Two personas** — pair-programming (conversation-first, asks when unsure) vs. autonomous (backlog-driven, never asks) are separate agents that no longer fight inside one prompt
- **Light** — Rust + Tauri: 5 MB installer, 19 MB binary, no Node runtime
- **Built for one person** — no enterprise features, no multi-tenant; the UX grows around its owner's habits

### Install

Download `kanzei-setup-*.exe` from [Releases](https://github.com/kanze1/kanzei-code/releases/latest) (per-user NSIS installer with uninstaller and Start Menu entry). The app silently checks for updates on startup; one-click update from Settings.

From source (dev machine):

```powershell
.\scripts\release.ps1            # test → build → install kz + kzapp to ~/.cargo/bin (with pending self-update)
.\scripts\package.ps1 -Publish   # build NSIS installer and publish to GitHub Releases
```

### Highlights

- **Three-protocol LLM layer**: Anthropic Messages / OpenAI Chat / OpenAI Responses, streaming state machines with proxy support (loopback exempt)
- **Subscription reuse**: Codex CLI login (`codex:gpt-5.6-*`), Claude Code long-lived token (`claude:*`), plus any OpenAI-compatible endpoint (Kimi, Ollama, ...)
- **Parallel read-only subagents** (`task` tool, fast/primary tiers) with live progress streaming to an activity panel
- **Dual agent personas**: pair-programming (conversation-first, default) vs. autonomous (backlog-driven with auto-continue), plus a research profile with enforced source citations
- **Markdown-native project tracking**: requirements / defects / goals as plain markdown with engine-enforced IDs, state machines, and archives — the agent develops kanzei itself off this backlog
- **Event-sourced sessions** (SQLite): steer/queue scheduling, restart recovery, conversation replay, automatic context compaction
- **Tauri desktop app**: conversation-first layout, activity sidebar, permission dialogs with persistent allow-rules, in-app doc viewer, priority/complexity-coded backlog

Chinese sections above are the source of truth; this summary tracks them loosely.
