# Kanzei 文档入口

先读 [当前实现与事实来源](current-state.md)，再按任务进入对应专题。本文是文档导航的唯一维护入口；旧 [目录](目录.md) 保留兼容链接。

| 要解决的问题 | 入口 |
| --- | --- |
| 当前产品有什么、代码和资产各归谁 | [当前状态](current-state.md)、[结构地图](structure-map.md) |
| 修改代码前的规则、验证与评审 | [Engineering rules](../AGENTS.md)、[贡献指南](../CONTRIBUTING.md) |
| 模块依赖、状态真源和调用方向 | [架构图](architecture)、[系统边界](architecture/00_system_boundaries.md) |
| 当前设计、约束与历史替代关系 | [专题合同](design/current_contracts.md)、[设计身份索引](../.kanzei/project/architecture/README.md)、[设计记录规范](design/readme.md) |
| 对话、项目与用户操作 | [使用手册](使用手册.md)、[熟悉的工作区](design/familiar_workspace.md)、[无项目对话](design/general_chat.md) |
| 项目知识、记忆来源和生命周期 | [项目知识](design/project_knowledge.md)、[记忆观测与效果](design/memory_feedback_reliability.md) |
| 网页、已有浏览器与原生窗口 | [浏览器合同](design/preview_pane.md)、[原生控制实现记录](reports/2026-10-19-native-control-experience.md) |
| 研究与实验 | [研究合同](design/research_mode.md)、[研究库](design/research_library.md)、[实验运行](design/research_experiment_runner.md) |
| 当前工作范围与剩余验收 | [需求](../.kanzei/project/requirements.md)、[缺陷](../.kanzei/project/defects.md)；状态与关闭由 tracker 维护 |
| 本地验证、打包和安装 | [发布证据合同](design/ci_release_evidence_chain.md)、[验证注册表](../scripts/verification-checks.json) |
| 下一阶段投入方向 | [2026-10-23 优化调研](reports/2026-10-23-optimization-research.md)；研究建议，不自动新增或关闭任务 |

## 如何判断文档是否适用

- 设计身份由索引标明：live_design 是现行合同，validated_design 保留其绑定版本的验证依据，historical_snapshot 和 superseded 不产生新的开发依赖。
- 源码与 Cargo 清单说明当前实现。定向测试、原生交互、真实模型评价、发布和安装是不同证据；报告中的旧通过结果只覆盖记录的版本与场景。
- reports/、reviews/、日期命名的根文档和 prototypes/ 保存历史观察。其文件数量、行号、待办和提交号不替代当前清单。10 月 2 日的[代码地图](reports/2026-10-02-code-map/README.md)属于历史快照；当前查找从结构地图和源码开始。
- 同一能力的现行合同维护在对应专题，历史报告只补日期明确的校正与指向，不再复制一套实时待办。

本次整理范围与检查见 [2026-10-23 文档维护记录](reports/2026-10-23-documentation-maintenance.md)。

工程修改遵守 [AGENTS.md](../AGENTS.md) 与 [CONTRIBUTING.md](../CONTRIBUTING.md)。
