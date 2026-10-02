# Sources

## S-001 R-221 研究模式设计基线：B4 回流与 B5 记忆一元化契约 [active]
- 查阅范围: §2 定调点4/5，§6 档位与工具面，§7 批4/批5，§8/§9/§10 回流、记忆和边界
- 用途: 核对 B4/B5 设计契约及验收要求
- 类型: 项目设计文档
- 证据锚: docs/design/research_mode.md:32-37,88-109,113-130
- 路径: docs/design/research_mode.md

## S-002 ResearchProfile 真实实现：回流权限与统一记忆入口 [active]
- 查阅范围: ResearchProfile::contribute 的 research 分支：source/finding 注册、memory_search/memory_note 放行、req/defect 受限 get/add、写权限与 git/bash 门禁、research/docs 注入及 research agent 提示词
- 用途: 核对 B4/B5 的真实工具实现、权限边界、统一记忆入口与历史 memory.md 禁止注入
- 类型: 仓库源码
- 证据锚: crates/kanzei-tools/src/profiles.rs:602-769
- 路径: crates/kanzei-tools/src/profiles.rs

## S-003 R-221 B5 实施提交 ecfdca5b：research 记忆统一走 memory 工具 [active]
- 提交: ecfdca5b
- 查阅范围: 提交标题与变更：R-221 B5 research 记忆统一走 memory 工具；核对 profiles.rs 中 B4 受限 tracker 权限及测试变更
- 用途: 提供提交级代码证据，确认 B4/B5 落地变更的版本锚点
- 类型: Git 提交
- 证据锚: ecfdca5b（提交标题：R-221 B5 research 记忆统一走 memory 工具）；关联变更文件：crates/kanzei-tools/src/profiles.rs

## S-004 R-221 B4/B5 真实研究计划 [active]
- 查阅范围: R-221 研究目标、B4/B5 检查点、执行步骤、停止条件
- 用途: 定义本次 B4/B5 研究边界与计划→来源→finding→report→todo 交付链路
- 类型: 研究计划
- 证据锚: .kanzei/research/r221-chain/plan.md:3-18,20-22
- 路径: .kanzei/research/r221-chain/plan.md
