# Kanzei 当前实现与文档口径

合同导航更新于 2026-10-22。实现以当前源码和 Cargo 清单为准；验证与发布状态读取绑定到相应提交的机器回执，本页不替代执行证据。

## 当前产品与运行合同

正式工作区采用「对话 / 管理」，项目内和无项目对话使用同级身份。管理提供需求、缺陷、交付和项目地图；记忆由全局入口管理，子代理和终端共用活动区，网页预览独立浮动。界面合同见 [熟悉的工作区](design/familiar_workspace.md)、[对话模式](design/conversation_modes.md) 与 [对话身份](design/conversation_peers.md)。旧工作台优先和 Softwire 总体草案已退出当前实施范围。

CLI 与桌面共用工具及运行装配。代码和用途指纹读取当前工作树，tracker、记忆和持久运行事实读取主项目根；界面状态、图与检索索引各自从真源派生。职责和消费边界见 [系统边界](architecture/00_system_boundaries.md)、[项目知识](architecture/03_project_knowledge.md) 与 [前后端协议](architecture/06_frontend_protocol.md)。

Skills 由全局 catalog 提供，启用技能正文按需读取；项目目录不参与发现，不按对话绑定正文。开发与研究的业务合同分别维护，语音使用当前本地 Faster-Whisper + Qwen3-TTS 路线；退出旧总控不表示未完成的原生设备或远程链路验证已经通过。

## 约束与工作范围

[当前约束](design/current_contracts.md) 导航项目规范、正式决策、动态验证策略和专题真源。[设计身份索引](../.kanzei/project/architecture/README.md) 区分现行设计、已验证设计、历史快照和替代关系。历史报告、原型以及归档条目的旧待办不自动生成当前任务或依赖。

活动需求和缺陷以 `.kanzei/project/requirements.md`、`defects.md` 为准；退役条目的原文、未完成验收和关闭原因保留在相应归档。当前需求的已完成前置转为 `refs`，只保留仍未满足的技术依赖；真实设备/SSH 待验收与未实现开发分别保留。

## 历史记录与验证

2026-10-03 至 10-04 的 WB1–WB6 逐文件审查属于截至点记录，见 [WB6 报告](reviews/file-audit-2026-10-03/WB6-delivery-activity.md) 与 [工作区验收](reviews/2026-10-04-ui-information-architecture/production-acceptance.md)。其中的文件数、下一包、待合并分支和安装进度不描述今天的排期。

本轮清理范围和验证结果见 [约束与需求清理记录](reports/2026-10-20-constraints-and-backlog-cleanup.md)。真实链路、安装发布和研究收益分别按专题证据记录。

运行装配由 `kanzei-runtime` 统一提供，CLI 与桌面直接依赖 `kanzei-memory`。`kanzei-tools` 提供能力，core 的存储实现模块不对外开放。贡献、测试隔离和历史保留规则见 [CONTRIBUTING.md](../CONTRIBUTING.md)。
