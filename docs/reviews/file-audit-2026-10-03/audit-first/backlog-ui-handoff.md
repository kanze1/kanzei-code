# 前端合并收口

- 对比基线：`86db2ccc`；审查开始 HEAD：`54b04021`。
- 另一工作树已移除，使用 `output/backlog-audit-2026-10-04/source.patch` 重建快照比较。主目录已有另一线全部 UI 改动；`25-softwire-view.js` 的空闲线路收起状态保护、overview 测试展开目标分组、runtime 的 audit-first 导入比快照更新，均保留。无需整文件替换。
- 比对记录 `output/backlog-merge-ui/snapshot-comparison.json` 是本次补修前的快照；最终改动文件哈希见同名 JSON。

## 已修复

**P1 / BACKLOG-UI-01**：批量状态菜单包含“待外部验收”，但原批量入口不提交后端必需的“外部验收”字段。即便条目已有实现与本地验证进展，也会被后端拒绝。

`11-docs-list.js` 现在同单条入口一样收集待验收事项；保留同时设置的标签及原批次项目归属。取消输入不写入。未绕过进展证据校验，未伪造任何进展。

本次实际修改只有 `crates/kanzei-app/ui/11-docs-list.js` 和 `scripts/ui-backlog-maintenance-smoke.mjs`。其他前端文件保留接管时已有改动；共享 `05-chat-render.js`、`06-activity.js` 和 `ui-runtime-smoke.mjs` 没有整文件替换。

## 验证

- 待办浏览器回归 **24 项通过**：分状态默认收起、筛选与刷新保留、类型/优先级/标签、取消原因、待外部验收展示与单条转入、需求退回 doing、缺陷退回 fixing、退回不启动对话、批量验收事项与标签并存、取消零提交，以及 60 条侧栏独立滚动和末项可达。
- overview **54 项通过**，spaces **38 项通过**。
- 已启动的完整 runtime 进程 **exit 0**，含 audit-first 回归及当时 15 项待办测试；最终 24 项版本另行通过，未重复整套 runtime。
- 修改文件 ESLint、`git diff --check` 通过。
- 已查看 light 与 dense 截图，状态/标签可见，右侧末项可达，输入框没有被长列表推出视口。

日志在 `output/backlog-merge-ui/`；截图和断言清单在 `output/playwright/backlog-maintenance/`。

## 边界

浏览器使用预览 IPC fixture，证明渲染、交互及传参；真实 tracker 持久化由后端任务验证。首次脚本启动曾因 node_modules 缺 playwright-core 失败，使用既有 lock 的 `npm ci --ignore-scripts --no-audit --no-fund` 恢复后通过；未修改依赖清单。

未运行 Cargo，未提交，未发布。最终提交后的完整 verify 由主任务执行。
