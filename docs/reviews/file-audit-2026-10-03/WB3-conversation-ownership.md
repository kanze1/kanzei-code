# WB3 通用对话、恢复与操作归属

日期：2026-10-03。基线 `26acb7f0`，保留 WB1/WB2 未提交修复及其他任务改动。本包新增全文审查 4 文件，相关共享 owner 只复核必要合同。

## 依赖地图

```text
general_chat.rs：独立根、关联复制；conversation_actions.rs：生命周期互斥、预览版本与回退/分叉
  → 03-general-scope：根身份、模式偏好；03-shell：当前项目/process/session
  → 09-sessions：processSwitchGeneration、列表与实际选择
  → 03-workspaces：创建与草稿归属；12-workbench：导航代次与项目激活
  → 15-views-misc：当前 pane 历史请求、窗口化缓存、只读查看器、新讨论入口
  → 05-conversation-actions：预览、回退/分叉及结果交付
  → 29-general-chat：无项目打开、新建、关联项目
```

状态真源仍在原模块：后端保存会话内容，shell/sessions 选择实际会话；workspaces 保存输入草稿；workbench 管导航。pane 恢复请求与只读 viewer 请求有各自的代次，不能互相借用。异步成功不表示其发起页面仍拥有当前输入框。

设计依据：[无项目对话](../../design/general_chat.md)、[会话状态合同](../../design/session_state_and_line_runtime.md)。保留源内 R-267/D-355 的 pane/项目隔离和新讨论只读语义；WB3 编号为本报告定位，不冒充登记 issue。

## crates/kanzei-app/ui/03-general-scope.js

### 职责
持有无项目根身份、当前对话根以及模式偏好的异步保存顺序。

### 判断
PASS

### 确切问题
- 未发现有实际影响的问题。

### 修改
- 无。

### 影响范围
- shell/workspaces、sessions、工作台与 general controller 的共享身份判断；Windows 路径表示与现有后端固定根一致，不扩展假设中的平台规则。

### 验证
- 全文及 callers 审查；根身份、空间/标签、普通打开与偏好不污染控制通过。

## crates/kanzei-app/ui/05-conversation-actions.js

### 职责
将消息上的预览、回退和分叉提交到原会话，并交付相应页面与输入框结果。

### 判断
P1

### 确切问题
- **WB3-01**：分叉 ACK、列表刷新、历史读取期间用户切走，迟到完成仍切到分叉或覆盖另一会话输入；同会话新输入也会被旧 prompt 覆盖。旧操作还会关闭后来重开的预览，迟到预览/失败提示会覆盖新页面。

### 修改
- 绑定原项目/process/session、选择代次、导航代次及本次预览；每个异步边界重验，自身分叉选择后改为核对实际目标与选择代次。
- 输入框只在仍匹配操作前快照时回填；失败显示同样守原归属。后端已完成的分叉保留，不擅自删除或回滚。

### 影响范围
- 唯一按钮生成 caller 为 05-chat-render；沿用 conversation_action 参数和 expectedHash。未改变后端回退、代码检查点或分叉存储格式。

### 验证
- 实际 Edge 的迟到预览、失败、分叉 ACK/刷新/历史、替换预览、保留新草稿和正常分叉回归；旧版本真实断言失败。

## crates/kanzei-app/ui/15-views-misc.js

### 职责
恢复与窗口化展示会话、维护历史列表和删除后视图，提供只读资源查看及新讨论入口。

### 判断
P1

### 确切问题
- **WB3-02 / P1**：旧历史请求失败把错误写进新会话；重试误走错误 pane 的缓存捷径，或刷新点击时的其他会话。同会话并发强制重载也缺少顺序，旧内容可覆盖新恢复结果。
- **WB3-03 / P2**：历史/关闭历史/文档查看器没有统一请求顺序，迟到结果覆盖后来打开的内容，甚至关闭后重新弹出。
- **WB3-04 / P1**：项目切换尚在进行时显示 A 的文档，切换完成后点击外部打开却使用全局 B 路径；失败重试也会读取后来 viewer 的目标。

### 修改
- 当前 pane 装载使用请求代次，并在失败、重试处保持项目/process/纪元合同；重试强制读取，不复用错误 pane。
- 所有 viewer 入口共享请求代次；新打开、关闭、导航或会话选择使旧请求失效。文档类型和项目一起保存，外部打开与重试固定原资源参数。
- 新讨论迟到导航的根因在 03-workspaces 修复，不在每个入口重复补丁。

### 影响范围
- callers 包括 sessions、运行恢复 28-async-workspace、回退操作、树/菜单、文档和资源预览。新增状态仅为运行时请求/显示归属；无 IPC 或持久化格式变化。

### 验证
- 实际 Edge 覆盖错会话失败、有效/过期重试、同会话重载乱序、viewer 乱序及关闭、文档跨项目外部打开。Git 状态刷新已有目标/代次守卫，历史删除继续按原 process/session 失效，无风格修改。

## crates/kanzei-app/ui/29-general-chat.js

### 职责
打开无项目会话、新建通用会话，并把原聊天关联复制到项目讨论。

### 判断
P1

### 确切问题
- **WB3-05 / P1**：新建会话期间用户选了别的会话或设置页，创建 ACK 仍接管选择；共享根因在 create_workspace_process。
- **WB3-06 / P1**：关联已成功、目标项目还在加载时，用户已选目标项目的另一会话；加载完成后 controller 强行选择关联结果。

### 修改
- 新建未取得选择权时停止后续加载/导航；关联在项目加载前后核对选择代次。

### 影响范围
- 侧栏无项目入口、统一新对话入口和关联菜单；不改变关联复制、原聊天保留和目标只读讨论语义。

### 验证
- 完整生产 general/workspace/sessions/workbench/history ESM，服务及 DOM 夹具；正常打开/新建/关联、早取消、错误释放、新会话在途取消均有控制。

## crates/kanzei-app/ui/03-workspaces.js（关联合同复核）

### 职责
创建会话并在原工作空间仍拥有选择权时切换，维护按目标隔离的草稿。

### 判断
P1

### 确切问题
- WB3-05 同一根因：原 same_context 只有项目/空间，不能识别同项目新选择或设置页导航。

### 修改
- 复用 workbench 导航代次和 sessions 选择代次；创建、列表刷新、选择加载各边界复核。自身 switchProcess 的代次单独认领，正常创建不会被误拒绝。

### 影响范围
- 全部 create_workspace_process callers 已搜索：sessions、15 新讨论、29 通用新建及 Softwire。不改函数签名或后端创建结果，不重复增加全文覆盖。

### 验证
- 正常通用新建、项目只读讨论、新选择/设置页取消、自身历史加载后被后来选择取代均测试。

## 验证结果

| 检查 | 当前结果 | 证据与边界 |
| --- | --- | --- |
| 通用会话/创建/关联 owner | 13 PASS | 完整生产 general/scope/sessions/workbench/workspaces/history ESM；外围服务和 DOM 夹具 |
| 历史与查看器 owner | 15 PASS | 实际 Edge、生产控制器与隔离 IPC；包括真实重试及回退按钮 |
| 回退/分叉操作 owner | 9 PASS | 实际 Edge；预览/ACK/刷新/历史各阶段交错、草稿及正常成功 |
| 精确旧源码负对照 | 预期断言失败 | general 10 PASS/3 FAIL；actions 2 PASS/7 FAIL；历史错误、viewer、外部打开、重载、实际回退分别负例失败。通过加载/浏览器路由替换旧源码，不切换工作目录 |
| 最终完整 UI runtime | exit 0 | 90 个生产 UI 模块、3606 次初始化 invoke、10 个主视图、0 运行时错误；既有浏览器套件及深浅主题 Harness 操作通过 |
| 全前端及修改脚本 ESLint | exit 0 | npm run lint 与定向脚本 lint |
| 原有 UI lint smoke | exit 0 | 179 文件、1646 导出 ESM guard，以及该脚本附带弹层/布局/背景浏览器检查 |
| diff、清单、源码指纹、引用 | PASS | 原 514 文件不重复，四块清单与队列一致，WB1/WB2/WB3 已审源码指纹一致 |

新增 **37 项**回归均接入 scripts/ui-runtime-smoke.mjs。独立检查与完整 gate 中重复运行的测试不重复计总量。整合第一遍因测试夹具漏新 import 和中间版夹具断言失败，未计为产品缺陷或通过；补齐夹具后最终完整 gate 重新执行并 exit 0，未跳过原测试。

证据集中于 `output/audit-WB3/`：`runtime-final.log`；`general/{current,old}-formal-final.log`、`general/verification.json`；`history/rewind-after/acceptance.json` 及 `before.log`、`viewer-before.log`、`document-baseline.log`、`reload-before.log`、`rewind-before.log`；`actions/{current,before}/acceptance.json` 和 `actions-before-final.log`。原 WB1 owner 测试只补导航/选择依赖夹具，17 项重新通过，行为断言未弱化。

本包新增全文 4 文件，全仓历史全文 **151/514**；当前四块 **76/268 已完成或可复用，192 待处理**，工作台 **28/51**。下一包 WB4：资源身份、状态文案、结构化解析与 Markdown 渲染。代码未提交、未发版。

# Module Summary

## 已修复
- P0：0。
- P1：5（WB3-01、02、04、05、06）。
- P2：1（WB3-03）。

## PASS 文件
- 03-general-scope.js。

## 仍需人工判断
- 无。

## 依赖影响
- 未改公共函数签名、IPC 参数、磁盘格式或数据库 schema。
- 在既有 owner 内补齐请求顺序和结果接收资格；已创建的会话不因取消 UI 接管而删除。

## 剩余风险
- 本轮以生产 ESM 和实际 Edge/隔离 IPC 验证，未重跑 Rust 全工作区或原生 WebView2 后端联调。无 Rust 修改，未发版。
- C7 原有未修复问题仍在第二块，不由本轮界面通过抵消。
