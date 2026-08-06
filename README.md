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

## 构建安装

```powershell
.\scripts\release.ps1   # test → release build → 安装到 ~/.cargo/bin
kz --version
```

## 使用

**桌面端(主要入口)**:`kzapp` — 类 VSCode 布局:项目管理 / 需求缺陷侧边栏(可展开、状态流转)/ 流式对话(工具块折叠)/ 模型直选 / 权限弹窗(拒绝/一次/总是)/ 运行日志面板 / 设置页。

**CLI**:`kz run "任务"`,`kz req|defect|source|finding list|add|...`(人用直通)。

**模型通道**(`~/.kanzei/kanzei.toml`):
- `codex:gpt-5.6-sol|terra|luna` — 复用 Codex CLI 订阅登录态(`~/.codex/auth.json`),自动刷新
- `ollama:<model>` — 本地模型,loopback 自动免代理
- 任意 Anthropic / OpenAI 兼容端点(自定义 provider + api_key_env)

**权限**:Ruleset last-match-wins,默认 ask;弹窗选"总是允许"会把泛化规则(bash 取命令首词前缀)写进项目 `.kanzei/kanzei.toml`。

## 进展

- ✅ M0/M0.5:LLM 协议层(Anthropic / OpenAI Chat / OpenAI Responses)+ 工具循环 + 代理
- ✅ M1 核心:harness 六注册表、双模式 profile(dev/research)、权限硬门禁、工具修复回路、req/defect/source/finding 追踪工具
- ✅ 桌面端一期/二期:见上
- ⏳ M2:SQLite 事件溯源、steer/queue 调度、压缩与 recall(**当前每条消息是独立任务,无跨消息会话记忆**)
- ⏳ M4:task 并行子代理(fast 角色跑)、MCP

设计规格参照:`docs/design/harness-m1.md` 与 `docs/reference/`(拷自 opencode 的 CONTEXT.md 与 specs/v2)。
