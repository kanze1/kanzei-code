# 架构与技术档案

本目录与 `requirements.md`、`defects.md` 同属项目管理资产，记录已经验证的架构边界、数据契约、运行流程和发布约束。

## 文档约定

- 一个主题一个 Markdown 文件，文件名使用 `snake_case`。
- 文档只记录当前已实现或已验证的事实；未完成内容标记为 `TODO`。
- 需求、缺陷和目标仍以同级追踪文档为真源，架构文档通过 ID 引用它们。

## 当前索引

- [`r030-process-decoupling.md`](../../../docs/design/r030-process-decoupling.md)：项目进程、会话和运行句柄隔离。
- [`r059-mobile-agent-communication.md`](../../../docs/design/r059-mobile-agent-communication.md)：主代理/子代理消息与通知演进设计。
- [`frontend-phase3.md`](../../../docs/design/frontend-phase3.md)：前端与后端能力对齐记录。
- [`interaction-modes.md`](../../../docs/design/interaction-modes.md)：交互模式与自动推进边界。
- [`reliability_usability_self_hosting_quality.md`](../../../docs/design/reliability_usability_self_hosting_quality.md)：可靠性、可用性与自举质量的统一不变量、验证证据和阶段门禁。
