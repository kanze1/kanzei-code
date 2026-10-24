# Kanzei 代码结构地图

2026-10-23 按当前工作树核对。本页维护职责与查找入口；文件数、行数和大小按需从源码计算，完整依赖以 Cargo 清单为准。历史计数不用于判断当前结构或决定拆分。

## 顶层职责

| 位置 | 内容与边界 |
| --- | --- |
| [crates](../crates) | Rust workspace、Tauri 入口与原生 ESM 前端 |
| [docs](README.md) | 现行合同、架构导航及注明时点的报告 |
| [scripts](../scripts) | 本地验证、构建、发布和 UI 回归；检查注册在 verification-checks.json |
| [.kanzei/project](../.kanzei/project) | tracker、决策、规范与设计身份；使用专用工具维护 |
| [.kanzei/memory](../.kanzei/memory) | 记忆 Markdown 真源；检索索引可重建 |
| [.kanzei/research](../.kanzei/research) | 项目研究来源、发现与工件；独立研究库依专题合同定位 |
| [tests/fixtures](../tests/fixtures) | 受版本管理的测试输入 |

target、dist、output、node_modules 以及运行数据库/临时工件不属于生产源码导航；清理遵守产物生命周期。各目录是否受版本管理以 Git 和 .gitignore 为准。

## Crate 的职责与依赖方向

| Crate | 职责与主要入口 |
| --- | --- |
| [kanzei-base](../crates/kanzei-base/src/lib.rs) | 原子文件、锁、路径、指纹、同步与测试原语 |
| [kanzei-llm](../crates/kanzei-llm/src/lib.rs) | 协议、流事件、认证、模型请求与用量 |
| [kanzei-harness](../crates/kanzei-harness/src/lib.rs) | agent/tool/skill/context/permission 合同、注册表与执行门禁 |
| [kanzei-core](../crates/kanzei-core/src/lib.rs) | 运行循环、会话与任务事实、压缩、恢复和领域服务；store 实现私有 |
| [kanzei-memory](../crates/kanzei-memory/src/lib.rs) | 记忆存储、检索、准入、生命周期、docstore 与回放评估 |
| [kanzei-tools](../crates/kanzei-tools/src/lib.rs) | 文件、Git、tracker、浏览器、研究与记忆整理能力 |
| [kanzei-runtime](../crates/kanzei-runtime/src/lib.rs) | CLI、桌面与定时执行共用的运行装配；组合能力，不被能力层反向依赖 |
| [kanzei](../crates/kanzei/src/cli/mod.rs) | kz CLI：输入、项目身份、工具直通、运行与收尾 |
| [kanzei-app](../crates/kanzei-app/src/main.rs) | kzapp：Tauri、IPC、桌面运行协调、窗口与 UI |

依赖方向和状态真源见 [系统边界](architecture/00_system_boundaries.md)。入口直接依赖所使用的能力 crate；tools 不再透传 memory/docstore/embed/replay_eval。memory 内的调度副本已删除，取活由调用方负责。编译检查与 [架构边界检查](../scripts/architecture-boundaries.mjs)共同维护这些约束。

## 按修改任务查找所有者

| 修改对象 | 所有者与入口 |
| --- | --- |
| 运行配置、模型请求装配 | [runtime](../crates/kanzei-runtime/src/lib.rs)、[CLI run](../crates/kanzei/src/cli/run.rs)、[桌面 assembly](../crates/kanzei-app/src/run/assembly.rs) |
| agent 循环与工具调度 | [runner/drive](../crates/kanzei-core/src/runner/drive.rs) 及同名目录；工具执行遵循 harness 契约 |
| 会话、任务、恢复与持久化 | core 公开领域服务；[桌面 persistence](../crates/kanzei-app/src/run/persistence.rs)；[任务投影](../crates/kanzei-core/src/store/task.rs)是内部实现导航 |
| 权限与敏感资源 | [permission](../crates/kanzei-harness/src/permission.rs) 及同名目录；实际执行入口消费同一合同 |
| 记忆来源、反馈和冷记忆 | [memory](../crates/kanzei-memory/src/memory/mod.rs)、[retention](../crates/kanzei-memory/src/memory/retention.rs)、[观测服务实现](../crates/kanzei-core/src/store/memory_observations.rs) |
| 对话做梦与整理作业 | [memory_dream](../crates/kanzei-tools/src/memory_dream.rs)、[memory_consolidation](../crates/kanzei-tools/src/memory_consolidation.rs) |
| 项目知识、用途与区域 | [project_knowledge](../crates/kanzei-tools/src/project_knowledge.rs)、[context](../crates/kanzei-tools/src/project_knowledge/context.rs)、[files](../crates/kanzei-tools/src/files.rs)、[areas](../crates/kanzei-harness/src/areas.rs) |
| tracker、改动归属与交付 | [tracker/actions](../crates/kanzei-tools/src/tracker/actions.rs)、[git/finalize](../crates/kanzei-tools/src/git/finalize.rs)、[work/reconcile](../crates/kanzei-tools/src/work/reconcile.rs) |
| 浏览器、网页与原生窗口 | [browser_tool](../crates/kanzei-tools/src/browser_tool.rs)、[browser helper](../scripts/browser-helper.mjs)、[preview](../crates/kanzei-app/src/preview/mod.rs)、[native_desktop](../crates/kanzei-app/src/native_desktop.rs) |
| 搜索、正文和研究证据 | [websearch](../crates/kanzei-tools/src/websearch.rs)、[webfetch](../crates/kanzei-tools/src/webfetch.rs)、[web_refs](../crates/kanzei-tools/src/web_refs.rs)、[research_verify](../crates/kanzei-tools/src/research_verify.rs) |
| 定时任务 | [runtime/schedules](../crates/kanzei-runtime/src/schedules.rs) 组装执行；[tools/schedules](../crates/kanzei-tools/src/schedules/mod.rs) 提供能力 |

## 前端与验证

前端是 [ui](../crates/kanzei-app/ui) 中的原生 ESM，由 [bootstrap.js](../crates/kanzei-app/ui/bootstrap.js) 装载。状态所有权、命令与事件以 [前后端协议](architecture/06_frontend_protocol.md) 为入口；不要根据文件序号推导运行顺序或自行复制状态。

检查入口统一为 [verify.ps1](../scripts/verify.ps1) 与 [verification-checks.json](../scripts/verification-checks.json)。浏览器内容合同由 ui_browser_contracts 统一运行，状态所有权由 ui_state_ownership 覆盖；真实桌面 UIA、真实后端与真实模型场景分别留证。正式发布对最终干净提交执行本地完整门禁，详见 [发布证据合同](design/ci_release_evidence_chain.md)。

查找实现时先用 rg/rg --files；需要完整依赖可运行 cargo metadata --no-deps --format-version 1。本文不维护第二份易漂移的完整文件清单。
