# Kanzei

个人 AI 编码与工作助手。采用精简共享 Rust 底座 + Tauri + 原生 ESM 纯中文桌面外壳，聚焦项目与无项目对话、外部记忆系统、隔离 Git 工作树、核心工具执行与对话伴生文件上下文追溯。

## 开始使用

从 [GitHub Releases](https://github.com/kanze1/kanzei-code/releases/latest) 下载安装器。桌面端安装在 `%LOCALAPPDATA%\kanzei\kzapp.exe`。选择项目或开始无项目对话，配置模型后输入任务。

支持 Codex 登录、Anthropic API Key、OpenAI 兼容服务及本地 Ollama。可用模型、权限和实际运行状态以当前配置及界面为准。

## 核心能力（精简共享底座）

- **共享底座**：`kanzei-base`、`kanzei-llm`、`kanzei-harness`、`kanzei-core`、`kanzei-memory`、`kanzei-tools`、`kanzei-runtime`、`kanzei`（CLI `kz`）、`kanzei-app`（桌面端 `kzapp`）。
- **记忆系统**：Markdown 真源、`DocStore`、混合检索与失败召回、记忆图谱（`refgraph`）、inbox 自动整理与 `.kanzei/memory/*` 硬保护。
- **核心工具**：文件读写编辑检索（`read`/`edit`/`insert`/`write`/`glob`/`grep`/`files`）、Shell 与后台进程（`bash`/`process`）、纯净 Git（含 `fmt`/`clippy` 门禁）与工作树隔离、沙箱（`sandbox`）、公网抓取与检索（`webfetch`/`websearch`）、外部 Playwright CDP 浏览器（`browser`）、轻量只读/可写子代理（`task`）。
- **对话伴生文件上下文抽屉**：在对话页右侧直接追溯本轮触及与 Git 改动的文件，支持对话内 `path:line` 就地定位、单文件 Git Diff 与带磁盘指纹校验的轻量编辑保存。
- **扩展与运维**：全局 Skills、自定义 Markdown Agent、极简 Goal 循环与应用内自动更新。

## 维护文档

| 文档 | 唯一职责 | 什么时候更新 |
| --- | --- | --- |
| [使用手册](docs/使用手册.md) | 用户如何完成常用操作 | 实际操作改变时 |
| [系统合同](docs/design/system_contracts.md) | 架构、状态真源、调用边界和源码入口 | 所有权或跨模块合同改变时 |
| [AGENTS](AGENTS.md) | Agent 工程规则 | 工程规则改变时 |
| [CONTRIBUTING](CONTRIBUTING.md) | 开发、测试与发布操作 | 工具链或门禁改变时 |

两份[架构文档](docs/architecture)说明模块边界与运行时状态机。许可证见 [LICENSE](LICENSE.md)。

## 开发与本地验证

安装 Rust、Windows 原生构建工具和 Node.js 后执行 `npm ci`。本地验证入口是 `scripts/verify.ps1`（全量门禁为 `scripts/verify.ps1 -Full`），详细步骤见 [CONTRIBUTING](CONTRIBUTING.md)。正式 UI 使用原生 ESM；React 仅用于开发测试夹具。
