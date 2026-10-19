# Kanzei 当前约束

- 身份：live_design
- 日期：2026-10-20
- 核对基线：df452a2cb79055ef7ce9b28a68bf989ba058fc23
- 关联需求：无；本页整理现有约束，不创建新的实施范围。
- 关联缺陷：无
- 关联决策：A-001、A-002、A-004、A-007、A-008、A-009、A-010

Kanzei 的当前约束按项目规则、正式决策和专题合同维护。旧二期总控、工作台优先和 Softwire 总体改造已退出本轮活动范围；各功能继续服从其现行合同和活动条目的原始验收。退役保留原因与未完成验收，不记为已完成交付。

## 规则来源

| 内容 | 真源与消费方式 |
| --- | --- |
| 用户当前任务与授权 | 当前对话；明确范围优先，已有授权继续有效 |
| 开发行为 | `crates/kanzei-tools/src/profiles/dev_system.md` |
| 项目规则的维护与来源 | `crates/kanzei-harness/assets/default_conventions.md` |
| 仓库专用规则 | `.kanzei/project/conventions.md`；全量载入并刷新 |
| Rust 提交门禁 | `crates/kanzei-harness/assets/cargo_conventions.md` 与 `crates/kanzei-tools/src/git.rs` |
| 验证和提交节奏 | 合并后的 `kanzei.toml [cadence]`；`profiles/policy.rs` 生成有效策略 |
| 长期决策 | `.kanzei/project/decisions.md`；只有 accepted 条目进入常驻约束目录 |
| 设计身份 | `.kanzei/project/architecture/README.md`；正文按需读取，superseded 排除默认装载 |
| 工作范围与验收 | `.kanzei/project/requirements.md`、`defects.md`；关闭和归档保留原验收与原因 |

## 专题合同

| 主题 | 当前入口 |
| --- | --- |
| 模块职责与调用方向 | `docs/architecture/00_system_boundaries.md`、`06_frontend_protocol.md` |
| Harness 与权限 | [Harness](harness_m1.md)、[自治与门禁](model_autonomy_and_harness_intensity.md) |
| 工作事实与验收 | [Work Unit](work_unit_foundation.md)、[Tracker 证据账本](tracker_evidence_ledger.md) |
| 会话与工作树 | [对话模式](conversation_modes.md)、[对话身份](conversation_peers.md)、[会话运行态](session_state_and_line_runtime.md)、[并行开发](deep_parallel_dev.md) |
| 子代理 | [三档协作](subagent_modes.md)、[呈现与归属](subagent_presentation.md) |
| 项目知识与记忆 | [项目知识](project_knowledge.md)、[记忆系统](memory_system.md)、[记忆事实](memory_feedback_reliability.md) |
| 当前界面 | [熟悉的工作区](familiar_workspace.md)、[对话呈现](chat_presentation_contract.md)、[弹层与框](ui_surface_stack.md) |
| 全局 Skills | [全局 Skills](global-skills.md)；项目目录不参与发现，不按对话绑定正文 |
| 文件与网页 | [文件编辑](files_editor.md)、[网页预览](preview_pane.md) |
| 研究与语音 | [研究合同](research_mode.md)、[实验运行](research_experiment_runner.md)、[本地语音](voice_interaction.md) |
| 质量与发布 | [质量不变量](reliability_usability_self_hosting_quality.md)、[发布证据链](ci_release_evidence_chain.md) |

源码和用途指纹读当前 `cwd`，项目 tracker、记忆与运行事实读主项目根。Markdown 保存治理目标和长期记忆，SQLite 保存原始运行事件，图和检索索引可重建；投影不能覆盖真源。权限与托管写入边界由代码执行。

## 依赖与历史

只有工作确实需要的未完成前置保留在 `依赖`。已经满足的前置作为 `refs` 留痕；历史队列顺序不伪装成技术依赖。dropped 条目退出开发队列，其原先未完成的验收不作为其它条目的已交付前置。

`historical_snapshot` 保留截至点的测量与设计事实。`superseded` 给出替代入口，正文保留供追溯。读取旧报告的文件数、测试数量、分支、安装版本和排期时使用其原日期，当前状态从实现和对应条目核对。

## 验证与产物

验证等级 E0–E4 和研究证据等级 V0–V3 分别使用。语法检查、局部测试、真实交互、外部服务与安装发布各自留证。发布清单以 `scripts/verification-checks.json` 和 `verify-policy.mjs` 为准，公开发布必须满足最终提交的全量证据。

开发依赖按真实调用方维护。React 仅用于浏览器表单验收夹具；它不是正式 UI 框架依赖。本地验证环境、证据和安装器按 `scripts/artifact-lifecycle.mjs` 回收；安装器在发布与安装验收后保留当前及上一版。

## 变更记录

- 2026-10-20：统一规则导航与专题所有权，明确退役、历史依赖、Skills 作用域和本地产物保留。

## 验证证据

规则装配见 `crates/kanzei-tools/src/profiles/dev.rs` 的 conventions、decisions、design-index 和 verification-policy 上下文源。设计身份由 `node scripts/check-design-freshness.mjs` 校验；本轮具体变更和验证结果见 `docs/reports/2026-10-20-constraints-and-backlog-cleanup.md`。
