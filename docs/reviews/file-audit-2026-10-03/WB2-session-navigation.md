# WB2 会话状态、列表与导航

日期：2026-10-03。起点 `26acb7f0`，保留 WB1 未提交修复；本包只处理四块计划中的工作台交互。

## 依赖地图

```text
后端 SessionRuntime / process_session_id / RunEvent
  → 01-core：按 session 路由事件，守住 stopping / converged
  → 03-session-stage：运行阶段及当前批次工具事件投影
  → 12-conversation-model：活动/关闭会话的统一身份和能力
  → 12-session-tree：本地/远端列表缓存、历史和会话打开
  → 12-session-menus：按原项目/process 提交管理动作
  → 12-workbench：浏览项目与执行项目的导航事务
```

消息归属由 `session_id` 决定，`processId` 是既有 IPC 定位符；项目概览的 browsingProject 不等于 currentProject。后者是依赖全局项目的编辑入口实际使用的执行根，不能把一次只读预览误当成已切换。

设计依据：[会话状态合同](../../design/session_state_and_line_runtime.md)、[工作台优先](../../design/workbench_first.md)。历史 R-197 / D-283 的状态与隔离原则继续适用；本包问题编号 WB2-01～03 仅为报告定位，不冒充已登记 issue。

## crates/kanzei-app/ui/03-session-stage.js

### 职责
把实际会话事件投影为阶段与说明，不决定会话是否结束。

### 判断
P2

### 确切问题
- **WB2-03**：并行工具先全部 ToolStart、再逐项 ToolEnd，首个结束就显示“等待模型”，但其他工具仍执行且 provider 尚不能继续；真实 ToolProgress 只有 id/chunk，还会清空原工具名。

### 修改
- 在所属 session state 内按工具 id 保存当前批次的名称投影；最后一个工具结束才显示等待模型，进度沿原 id 找名称。
- turn/轮末/会话终态清掉旧批次；终态标签仍归 session 状态机。没有全局工具 Map、磁盘格式或调度器变化。

### 影响范围
- 唯一 stage writer caller：01-core；compactionInProgress 另供 Softwire。09-sessions 的阶段与 tooltip、工作台运行投影消费结果。

### 验证
- 已核真实 parallel_tools → FuturesUnordered → ToolEnd 顺序和 ToolProgress payload；新回归验证双工具、最后结束、名称保留、会话隔离、下一轮重置及终态清理。最终执行结果见验证表。

## crates/kanzei-app/ui/12-conversation-model.js

### 职责
统一活动执行与保留历史的会话身份、能力、搜索和时间字段。

### 判断
PASS

### 确切问题
- 未发现有实际影响的问题。

### 修改
- 无。

### 影响范围
- 唯一生产 caller 为 session-tree；活动记录先于 closed 按 session identity 去重。关闭只取消执行能力，不取消 rename/delete。无兼容性变化。

### 验证
- 全文与 caller 检查；后端时间字段为毫秒，session_id 由项目根与 process prefix 确定。无可达的同项目/process 身份轮换，不把缓存签名的理论疑点计为 bug。

## crates/kanzei-app/ui/12-session-tree.js

### 职责
持有远端/关闭会话查询缓存，绘制会话树与历史，并编排明确的会话打开动作。

### 判断
P1

### 确切问题
- **WB2-02**：openSession 等待 switchProcess 的历史读取时，用户已去设置或工作台；迟到完成仍 ensureChatView，把用户拉回对话并返回 true。调用方可能继续打开原会话的交付表单。

### 修改
- 完成前复核导航代次、页面、项目、空间和进程。被新选择取代或目标不存在时返回 false，不再执行后续跳页。

### 影响范围
- 直接 caller：侧栏/历史激活、命令面板、菜单与交付入口。使用既有 workbenchNavigationGuard，不另建导航状态机；API 返回值恢复为实际成功含义。

### 验证
- 真实 Edge 旧负例得到 `view=chat, result=true`；修复后 8 项通过：双会话乱序、设置/工作台取消、正常成功、不存在进程返回 false 且不改变目标。

## crates/kanzei-app/ui/12-session-menus.js

### 职责
将会话及项目右键、键盘、排序、重命名、关闭与删除动作交给既有 owner。

### 判断
PASS

### 确切问题
- 未发现独立有实际影响的问题；openSession 的迟到成功在 tree owner 修复，不在各个菜单 caller 叠判断。

### 修改
- 无。

### 影响范围
- 复核 lifecycle 的 rename/purge/close、09-sessions 的关闭收尾、15-views-misc 的历史删除、tree 的活动/关闭去重和首选项。无持久格式或调用协议变化。

### 验证
- 全文、事件委托和拖动生命周期审查；菜单操作保持原 project/process，关闭按后端 ProcessHandle 定位。关联导航及原有会话隔离回归覆盖实际调用。

## crates/kanzei-app/ui/12-workbench.js

### 职责
协调项目预览、执行根选择、页面导航与上下文资源入口。

### 判断
P1

### 确切问题
- **WB2-01**：A 是执行项目、B 只是概览时，打开 B 的规范仅再次预览 B；openConventions 仍捕获 currentProject=A，读取和后续保存指向 A。

### 修改
- 规范入口通过既有 activate 事务完成目标项目切换，再复核 currentProject。中途取消时不打开对话框。

### 影响范围
- 直接 caller：Softwire resource 动作；沿 enterProject → activate_execution_root 与规范 dialog 的 project/expectedHash 保存合同修复。只读概览仍不自动激活；没有改规范存储 API。

### 验证
- 旧 Edge 确认 B 入口实际读取 A；修复后 7 项通过，包括 B 读取/保存、保持概览、中途转去 B 时取消 A 的打开、全程不调用 run_prompt。

## crates/kanzei-app/ui/01-core.js（关联合同复核）

### 职责
按 session 路由事件并更新统一运行状态，再分发前后台渲染。

### 判断
P2

### 确切问题
- WB2-03 同一根因的清理边界：仅在未收敛且非 stopping 时调用 stage helper，会漏掉停止和已收敛会话的批次清理。

### 修改
- 仅允许轮末/终态进入 helper 做清理；迟到进度仍受原 stopping/converged 守卫约束，终态 phase/label 仍由现有状态机维护。

### 影响范围
- 不重复计算问题或已审文件；不改事件 payload，不改前后台消息路由。

### 验证
- 使用实际 core 事件入口验证 stopping → idle/stopped/error 的清理和迟到进度不复活；结果见下表。

## 验证

| 检查 | 本轮结果 | 验证边界 |
| --- | --- | --- |
| 新阶段/状态回归 | 21 PASS | 纯函数及完整生产 01-core/03-shell ESM，外围服务和 DOM 使用夹具 |
| 新会话导航回归 | 8 PASS | 实际 Edge、生产控制器、隔离 IPC |
| 新项目资源归属回归 | 7 PASS | 实际 Edge、生产规范读写入口、隔离 IPC |
| 精确旧源码负对照 | 预期失败 | 阶段 15 PASS/6 ERR_ASSERTION；导航被拉回 chat；B 规范入口读到了 A。未通过修改工作目录切换旧版本 |
| 原会话 identity 回归 | 22 PASS | 生产会话选择和导航模块、受控 IPC |
| 原 Softwire 概览回归 | 54 PASS | 实际 Edge；另有工作台浏览器回归通过 |
| 完整 UI runtime gate | exit 0 | 90 个生产 UI 模块、3608 次初始化 invoke、10 个主视图、0 运行时错误；包括新增三脚本及已有编辑器、对话、Softwire 等浏览器套件；深浅主题 Harness 操作均通过 |
| 全前端 lint、新脚本 lint、diff-check | exit 0 | 静态检查和空白检查 |

新增回归共 **36 项**，已接入 scripts/ui-runtime-smoke.mjs。上表独立运行的套件有部分也被完整 gate 再次运行，不重复相加为总测试量。

证据：`output/audit-WB2/runtime.log`、`output/audit-WB2/model/verification.json`、`output/audit-WB2/model/{old,current}-formal-first.log`、`output/audit-WB2/tree/navigation-owner/acceptance.json`、`output/audit-WB2/tree/navigation-before-rerun.log`、`output/audit-WB2/resources/acceptance.json`、`output/audit-WB2/resources-before-repeat.log`。三个新增脚本均支持 `--before` 精确旧源码负对照。

本包新增全文审查 5 文件；01-core 和规范入口只计相关合同复核。全仓历史全文累计 **147/514**；当前四块 **72/268 已完成或可复用，196 待处理**，其中工作台 **24/51**。下一包 WB3 沿 general scope → 历史/对话操作 → 无项目界面入口推进。

# Module Summary

## 已修复
- P0：0。
- P1：2，跨项目规范路由、迟到会话导航及错误成功回执。
- P2：1，并行工具阶段/名称投影。

## PASS 文件
- 12-conversation-model.js、12-session-menus.js。

## 仍需人工判断
- 无。

## 依赖影响
- 沿用现有执行根激活和导航守卫；openSession 被取消/无目标返回 false。
- stage_tools 是所属会话的运行时派生投影，不是持久任务表；终态清理接入原事件 owner。

## 剩余风险
- 浏览器验证使用隔离 IPC，没有重新运行 Rust 全工作区或原生 WebView2 后端联调；本轮没有 Rust 修改、数据库格式变化、发布或用户项目数据操作。
- C7 原有 3 个未修复 P1 仍列第二块，不因本轮 UI 通过而标为完成。
