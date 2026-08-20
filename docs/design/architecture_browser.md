# 可视化架构浏览与维护记忆设置——技术栈选型评估报告

- 身份：validated_design
- 状态：已实施；R-122 已完成，D-173 已修复
- 日期：2026-08-10
- 最近核验提交：5c9e1df
- 关联需求：R-122 (done)
- 关联缺陷：D-173 (fixed)
- 关联决策：A-008 A-007

## 背景与问题（实施前输入）

R-122（2026-08-08 用户）：缺少架构浏览入口，要求「可视化做好一点，和设置记忆这些同级目录，要慎重选取技术栈」。验收三条：①实现可视化架构图/浏览器；②支持维护记忆等配置信息；③完成技术栈选型评估报告（本条即③）。

## 实施前调研事实（2026-08-10，非当前状态）：

- 架构数据真源是 `.kanzei/project/architecture/README.md` 索引（architecture 工具维护）+ `docs/design/*.md` 设计文档；索引当前有 3 个 design doc 未入册（D-173 缺口）。
- 前端约束：A-008（decisions.md:52-57）——有序 classic script，不引入 ES modules、打包器或框架；`ui/` 无 package.json/构建工具，`index.html` 690-707 行按序加载 18 个 classic script。
- 已有可复用渲染模式：`17-files.js` buildFilesTree/renderFilesDir（嵌套可折叠目录树，R-148 交付）；`13-memory.js` renderMemoryArch（记忆架构总览卡片）；应用内 Markdown 查看器 openDocViewer。
- 记忆维护命令齐备：memory_overview/memory_entries/memory_entry_save/memory_entry_delete/memory_consolidate/memory_focus_set 等 13 个 Tauri command（memory.rs），前端无需再造后端。

## 目标与非目标

目标：

- 架构浏览与设置、记忆同级入口（主导航新视图），可视化呈现 architecture 索引 + docs/design 文档树。
- 维护记忆等配置信息：在架构浏览/记忆页触达既有 memory_* 命令（编辑、整理、重心设置）。
- 交付技术栈选型评估报告（本条）。

非目标：

- 不实现图形化 DAG/拓扑画布——与 R-111 同判据：列表+层级树已覆盖主要浏览场景，图形画布重且收益存疑（若用户后续明确要图，另立条目）。
- 不引入构建步骤、不引入 ES modules、不新增运行时依赖（A-008 硬约束）。

## 讨论摘要

2026-08-08 用户原话「要慎重选取技术栈」的背景是：项目此前在 i18n 上吃过大亏（词典替换 MutationObserver 家族缺陷 D-092 等 8 条），方向基线 A-007 明确「可替代区复刻、创新只投护城河」。因此选型的首要判据不是「最新最强」，而是「与既有架构一致性最高、增量风险最低」。

## 候选方案

| 方案 | 技术栈 | 增量 | 违背 A-008？ | 主要风险 |
| --- | --- | --- | --- | --- |
| A | 既有 classic script + 目录树渲染（复用 17-files.js 模式） | 新 Tauri command 读架构索引 + 新视图 JS | 否 | 无构建、无新依赖 |
| B | 引入图表库（mermaid/d3）画架构图 | 需 CDN/本地 vendored 库 | 是（新依赖） | 图形收益存疑；vendored 库体积；与 A-007 相悖 |
| C | 引入前端框架（React/Vue）重做导航 | 需构建工具链 | 是（A-008 明文禁止） | 全量重构成本；与巨石拆解方向相悖 |
| D | 独立 web 应用/服务 | 新工程 | 是 | 与桌面端架构割裂，双份维护 |

## 最终方案

**已实施方案 A**：架构浏览作为主导航新视图（`view-arch`），数据来自只读 Tauri command（读取 architecture 索引与 docs/design 目录清单，输出带状态分组的文档树），前端复用 17-files.js 的目录树渲染模式 + 应用内 Markdown 查看器打开设计文档；记忆配置触达既有 memory_* 命令（记忆页与架构浏览同级入口，不重复造后端）。该方案已由 R-122 批2/3 交付，以下取舍记录保留为实施依据。

选型理由：

1. **A-008 是已接受的长期决策**：有序 classic script、无构建步骤，是前端三阶段拆解（R-154~R-156）刚交付的现状，任何新栈都推倒重来。
2. **数据已在 markdown 且已有维护通道**：architecture 索引有专用工具，docs/design 有规范模板；浏览视图只做只读呈现，不需要新数据层。
3. **渲染模式现成**：R-148 的目录树与 R-092 的 Markdown 查看器都是刚验证过的既有能力，复用成本最低。
4. **记忆维护已有 13 个命令**：架构浏览/记忆页只做消费方，符合「声称的能力必须有真实调用方」的验收口径。

## 技术选型与取舍

- 取舍：不做图形 DAG（列表+树已覆盖；与 R-111 同判据）。若用户实测后仍要图形化，单独立项评估 mermaid 等库的 vendored 成本。
- 影响范围：前端 ui/ 新增 1 个视图脚本 + 少量 i18n 词条 + style.css；kanzei-app 新增 1 个只读 command；不触碰 runner/tools/memory 后端。
- 验证方式：node --check + 四条前端冒烟（runtime smoke 断言树渲染/展开/打开查看器/记忆入口可见）；kanzei-app 定向单测覆盖新 command 的索引解析。

## 实施边界与调用方

- 新增 command：`architecture_snapshot`（只读，读索引+目录，返回分层树数据），调用方为架构浏览视图。
- 架构浏览视图挂到主导航（与设置、记忆同级），复用 `switchView` 模式。
- 记忆配置：视图内「记忆」入口跳转 `#view-memory`，维护动作全部走既有 memory_* 命令（既有能力，非本次交付）。
- 顺带收口 D-173 的索引缺口：新文档入册时用 architecture 工具 update，同时把已存在的 3 个未入册 design doc 补进索引。

## 变更记录

- 2026-08-10 创建：R-122 批1 技术栈选型评估报告。

## 验证证据

- 批1：本报告（验收③）。索引缺口修复由 architecture check 复验为 0 issue。
- 批2/批3：前端冒烟断言 + kanzei-app 定向单测（见 R-122 进展）。

## 后续边界

- 实施前输入：`direction_taste.md` §6 曾建议冻结 R-122；该建议未成为当前 tracker 状态，R-122 已按三批交付并关闭。
- 当前边界：图形化架构图不在 R-122 范围；若用户后续明确需要图形拓扑，另立条目评估 mermaid/d3 的 vendored 体积与 A-007 判据。
