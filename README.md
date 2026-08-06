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

## 使用(M0)

```powershell
# 本地模型(Ollama)
$env:KANZEI_PROVIDER = "ollama"; $env:KANZEI_MODEL = "qwen3"
kz run "读取 Cargo.toml,列出 workspace 的 crate"

# Anthropic
$env:ANTHROPIC_API_KEY = "..."
kz run "..."
```

设计规格参照:`docs/reference/`(拷自 opencode 的 CONTEXT.md 与 specs/v2)。
