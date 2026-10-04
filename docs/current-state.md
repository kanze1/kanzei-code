# Kanzei 当前实现与文档口径

2026-10-04 工作区改造：正式前端已采用「对话 / 管理」，管理包含需求、缺陷、交付、项目地图；全局记忆统一入口，子代理/终端共用活动栏，网页预览独立浮动。研究空间和想法退出导航。当前实现、数据口径和试用验收见 [工作区设计](design/familiar_workspace.md) 与 [改造验收](reviews/2026-10-04-ui-information-architecture/production-acceptance.md)。发布与安装状态以对应提交的发布回执为准；下面的文件审查进度仍是独立工作。

> 当前审查计划（2026-10-03）：仅工作台与交互控制台、对话执行与恢复、工具与代码操作、多任务与自动运行，按此顺序。记忆、科研、任务管理/自举、手机语音及配置升级专项已移出执行范围。见 [四块审查计划](reviews/file-audit-2026-10-03/parallel-plan.md)；下面的全产品实现说明不代表审查待办。

当前审查进度（2026-10-04）：WB1–WB6 累计新增全文审查 27 文件。WB6 修复附件时长误读、旧交付回执覆盖、卡片大小不刷新、侧栏交互归属和活动表现顺序共 6 个问题，58 项新增回归、Rust app 检查/直接 caller 与完整 UI gate 通过；四块已完成/可复用 88/269，剩余 181 文件，下一包 WB7（项目概览后端 → 项目会话列表/异步工作台）。WB1–WB6 已提交为 9875810c，正在合并远端 c6654fae 并进行最终发布验证；详见 [WB6 逐文件报告](reviews/file-audit-2026-10-03/WB6-delivery-activity.md)，前轮见 [WB5](reviews/file-audit-2026-10-03/WB5-rendering-ownership.md)。

对话语义以 [统一模型](design/conversation_peers.md) 为准：对话同级、新建默认 dev、readonly 显式选择，操作指定真实 process_id；WB 修复继续保护原请求和会话归属。新增测试文件已计入待审，合并不虚增审查完成数。发布整合见 [报告](reports/2026-10-04-workbench-release.md)。
