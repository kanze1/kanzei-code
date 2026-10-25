# Kanzei

个人 AI 工作与开发工具。使用 Rust、Tauri 和原生 ESM，支持项目与无项目对话、外部记忆、隔离工作树、工具执行和可追溯交付。

## 开始使用

从 [GitHub Releases](https://github.com/kanze1/kanzei-code/releases/latest) 下载安装器。桌面端安装在 `%LOCALAPPDATA%\kanzei\kzapp.exe`。选择项目或开始无项目对话，配置模型后输入任务。

支持 Codex 登录、Anthropic API Key、OpenAI 兼容服务及本地 Ollama。可用模型、权限和实际运行状态以当前配置及界面为准。

## 只维护这些文档

| 文档 | 唯一职责 | 什么时候更新 |
| --- | --- | --- |
| [使用手册](docs/使用手册.md) | 用户如何完成常用操作 | 实际操作改变时 |
| [系统合同](docs/design/system_contracts.md) | 架构、状态真源、调用边界和源码入口 | 所有权或跨模块合同改变时 |
| [管理工作区方案](docs/design/management_workspace.md) | 尚未实施的下一版管理设计 | 方案改变或实施后收敛时 |
| [AGENTS](AGENTS.md) | Agent 工程规则 | 工程规则改变时 |
| [CONTRIBUTING](CONTRIBUTING.md) | 开发、测试与发布操作 | 工具链或门禁改变时 |

本页负责产品说明与导航。两份[架构文档](docs/architecture)服务应用内架构浏览；[运行时状态机](docs/architecture/01_runtime_loop.md)统一维护执行层级、会话监督器、状态转换、资源回收和验证边界。许可证见 [LICENSE](LICENSE.md)。需求、缺陷、验收和长期决策保留在 `.kanzei/project/`，它们是项目数据，不再复制成设计进度报告。

## 旧资料

2026-10-24 停止维护原设计分册、审查、报告、原型和素材说明。已提交版本从 Git 历史恢复，清理前基线为 `a667ffbe`。需要完整导出该版本的旧文档时，可在仓库内执行 `git archive --format=zip --output=../kanzei-docs-a667ffbe.zip a667ffbe docs/`。该入口只恢复已提交内容，不代表未提交原稿已另行备份。

旧台账正文和源码注释中的文档路径保留为历史定位，不再构成现行合同。需要查原方案时，在独立目录恢复对应 Git 版本或解压归档；日常开发从系统合同和当前台账进入。新增内容优先写回上述文档，不再维护逐次审查、交接、版本状态和重复目录。

## 开发

安装 Rust、Windows 原生构建工具和 Node.js 后执行 `npm ci`。本地检查入口是 `scripts/verify.ps1`，详细步骤见贡献指南。正式 UI 使用原生 ESM；React 仅用于开发测试夹具。

## English

Kanzei is a personal AI work and coding tool built with Rust, Tauri and native ESM. It combines project and general conversations, external memory, isolated Git worktrees, tool execution and traceable delivery. Install it from GitHub Releases; the documents above cover usage, architecture, the pending management design, engineering rules and local verification.
