# 工作台、核心工具执行与相邻规范链只读审计

基线：`55eaca24750ac431ebe7f577c2f4b7b8cbded6f5`。44 文件全文审查，其中 4 文件由相邻代理全部或部分协审；原根组 38，追加工具组架构/规范 6。仅审计，产品修改 0。完整机器记录见 [root.json](root.json)。

依赖链：core 工具输出/批次 → tools 文件/规范/架构 → app 工作树与交付/预览 → UI 卡片与操作。SQLite 是会话/交付事实源；规范和索引文件由持锁 CAS 写者拥有；预览 Pane/无头实例与聊天 composer 的 owner 必须分别绑定。

历史合同：R-250（schema）、R-310（工具遥测）、D-398（写日志）、D-755（架构索引只拒新增问题）、UI2-0926 #8。参考 [预览设计](../../../design/preview_pane.md) §5 和 [架构图设计](../../../design/architecture_diagrams.md)。

## crates/kanzei-app/src/commands/os_open.rs

### 职责
解析对话目录，安全构造外部程序启动参数和打开方式配置。

### 判断
PASS

### 确切问题
- 未发现有实际影响的问题。

### 修改
- 未修改：本轮先审完再集中修复。

### 影响范围
- 已核对模块内调用、直接调用边界及相邻 owner；未提出 API、持久格式或兼容性变更。

### 验证
- 全文及现有测试断言审阅；未新增或运行该文件专项测试，不将静态 PASS 解释为全场景运行证明。

## crates/kanzei-app/src/commands/run/tool_process.rs

### 职责
停止指定项目的单个后台工具进程。

### 判断
PASS

### 确切问题
- 未发现有实际影响的问题。

### 修改
- 未修改：本轮先审完再集中修复。

### 影响范围
- 已核对模块内调用、直接调用边界及相邻 owner；未提出 API、持久格式或兼容性变更。

### 验证
- 全文及现有测试断言审阅；未新增或运行该文件专项测试，不将静态 PASS 解释为全场景运行证明。

## crates/kanzei-app/src/deliveries.rs

### 职责
持久化文件交付收据并按真实文件版本投影交付列表。

### 判断
PASS

### 确切问题
- 未发现有实际影响的问题。

### 修改
- 未修改：本轮先审完再集中修复。

### 影响范围
- 已核对模块内调用、直接调用边界及相邻 owner；未提出 API、持久格式或兼容性变更。

### 验证
- 全文及现有测试断言审阅；未新增或运行该文件专项测试，不将静态 PASS 解释为全场景运行证明。

## crates/kanzei-app/src/files_view.rs

### 职责
文件快照、预览和用途标注的应用层接口。

### 判断
PASS

### 确切问题
- 未发现有实际影响的问题。

### 修改
- 未修改：本轮先审完再集中修复。

### 影响范围
- 已核对模块内调用、直接调用边界及相邻 owner；未提出 API、持久格式或兼容性变更。

### 验证
- 全文及现有测试断言审阅；未新增或运行该文件专项测试，不将静态 PASS 解释为全场景运行证明。

## crates/kanzei-app/src/harness_ext.rs

### 职责
装配 UI 自查、文件交付和桌面 browser 工具。

### 判断
PASS

### 确切问题
- 未发现有实际影响的问题。

### 修改
- 未修改：本轮先审完再集中修复。

### 影响范围
- 已核对模块内调用、直接调用边界及相邻 owner；未提出 API、持久格式或兼容性变更。

### 验证
- 全文及现有测试断言审阅；未新增或运行该文件专项测试，不将静态 PASS 解释为全场景运行证明。

## crates/kanzei-app/src/preview/agent.rs

### 职责
串行执行模型发起的可见面板浏览器动作。

### 判断
P1

### 确切问题
- AF-R09 等到面板动作锁后未重查绑定会话（26-42）：A 的 browser 动作已选中面板，等待另一个父/子代理动作释放 agent 锁；用户切换到 B，可见性命令将面板绑定 B。A 后续获得锁仍操作当前 B 面板。服务桥只比 URL，同页切会话无法拦截。

### 修改
- 未修改：本轮先审完再集中修复。
- 后续最小修复：动作锁内复核 pane generation、process owner 和页面身份；与无头浏览器 owner 一起修。

### 影响范围
- route_waiting 在锁外；set_visible 不取 agent 锁；execute_checked 锁内只检查 visible/expected_url，没有 process owner。设计 preview_pane.md §5 要求当前绑定本线。

### 验证
- AF-R09：静态交错证据：harness_ext.rs:395、desktop_bridge.rs:194、preview/agent.rs:34-42、preview/pane.rs:set_visible、24-preview.js:previewLineSync；静态调用与可达异步交错，未做原生面板动态复现。

## crates/kanzei-app/src/preview/cdp.rs

### 职责
主线程 WebView2 CDP 往返、事件订阅和导航闸。

### 判断
PASS

### 确切问题
- 未发现有实际影响的问题。

### 修改
- 未修改：本轮先审完再集中修复。

### 影响范围
- 已核对模块内调用、直接调用边界及相邻 owner；未提出 API、持久格式或兼容性变更。

### 验证
- 全文及现有测试断言审阅；未新增或运行该文件专项测试，不将静态 PASS 解释为全场景运行证明。

## crates/kanzei-app/src/preview/commands.rs

### 职责
前端预览 IPC 到面板 owner 的适配层。

### 判断
PASS

### 确切问题
- 未发现有实际影响的问题。

### 修改
- 未修改：本轮先审完再集中修复。

### 影响范围
- 已核对模块内调用、直接调用边界及相邻 owner；未提出 API、持久格式或兼容性变更。

### 验证
- 全文及现有测试断言审阅；未新增或运行该文件专项测试，不将静态 PASS 解释为全场景运行证明。

## crates/kanzei-app/src/preview/console.rs

### 职责
CDP 事件解析与有界、序号单调的控制台缓冲。

### 判断
PASS

### 确切问题
- 未发现有实际影响的问题。

### 修改
- 未修改：本轮先审完再集中修复。

### 影响范围
- 已核对模块内调用、直接调用边界及相邻 owner；未提出 API、持久格式或兼容性变更。

### 验证
- 全文及现有测试断言审阅；未新增或运行该文件专项测试，不将静态 PASS 解释为全场景运行证明。

## crates/kanzei-app/src/preview/host.rs

### 职责
原生主窗口和子 webview 的焦点、移动通知。

### 判断
PASS

### 确切问题
- 未发现有实际影响的问题。

### 修改
- 未修改：本轮先审完再集中修复。

### 影响范围
- 已核对模块内调用、直接调用边界及相邻 owner；未提出 API、持久格式或兼容性变更。

### 验证
- 全文及现有测试断言审阅；未新增或运行该文件专项测试，不将静态 PASS 解释为全场景运行证明。

## crates/kanzei-app/src/preview/mod.rs

### 职责
预览状态、路由、地址规则与导航状态机。

### 判断
PASS

### 确切问题
- 未发现有实际影响的问题。

### 修改
- 未修改：本轮先审完再集中修复。

### 影响范围
- 已核对模块内调用、直接调用边界及相邻 owner；未提出 API、持久格式或兼容性变更。

### 验证
- 全文及现有测试断言审阅；未新增或运行该文件专项测试，不将静态 PASS 解释为全场景运行证明。

## crates/kanzei-app/src/preview/pane.rs

### 职责
原生面板创建、关闭、显示、设备、截图和批注生命周期。

### 判断
PASS

### 确切问题
- 未发现有实际影响的问题。

### 修改
- 未修改：本轮先审完再集中修复。

### 影响范围
- 已核对模块内调用、直接调用边界及相邻 owner；未提出 API、持久格式或兼容性变更。

### 验证
- 全文及现有测试断言审阅；未新增或运行该文件专项测试，不将静态 PASS 解释为全场景运行证明。

## crates/kanzei-app/src/preview/pure_tests.rs

### 职责
预览路由、导航、布局和安全边界的纯逻辑测试。

### 判断
PASS

### 确切问题
- 未发现有实际影响的问题。

### 修改
- 未修改：本轮先审完再集中修复。

### 影响范围
- 已核对模块内调用、直接调用边界及相邻 owner；未提出 API、持久格式或兼容性变更。

### 验证
- 全文及现有测试断言审阅；未新增或运行该文件专项测试，不将静态 PASS 解释为全场景运行证明。

## crates/kanzei-app/src/processes/gate.rs

### 职责
在工作树执行项目可用的收尾检查。

### 判断
PASS

### 确切问题
- 未发现有实际影响的问题。

### 修改
- 未修改：本轮先审完再集中修复。

### 影响范围
- 已核对模块内调用、直接调用边界及相邻 owner；未提出 API、持久格式或兼容性变更。

### 验证
- 全文及现有测试断言审阅；未新增或运行该文件专项测试，不将静态 PASS 解释为全场景运行证明。

## crates/kanzei-app/src/processes/workspace.rs

### 职责
工作树收割、合并和合并后会话处理。

### 判断
PASS

### 确切问题
- 未发现有实际影响的问题。

### 修改
- 未修改：本轮先审完再集中修复。

### 影响范围
- 已核对模块内调用、直接调用边界及相邻 owner；未提出 API、持久格式或兼容性变更。

### 验证
- 全文及现有测试断言审阅；未新增或运行该文件专项测试，不将静态 PASS 解释为全场景运行证明。

## crates/kanzei-app/src/screenshot.rs

### 职责
原生窗口像素抓取、空图判定与 PNG 编码。

### 判断
PASS

### 确切问题
- 未发现有实际影响的问题。

### 修改
- 未修改：本轮先审完再集中修复。

### 影响范围
- 已核对模块内调用、直接调用边界及相邻 owner；未提出 API、持久格式或兼容性变更。

### 验证
- 全文及现有测试断言审阅；未新增或运行该文件专项测试，不将静态 PASS 解释为全场景运行证明。

## crates/kanzei-app/src/verification_monitor.rs

### 职责
将后台验证终态幂等投递到原会话的持久队列。

### 判断
PASS

### 确切问题
- 未发现有实际影响的问题。

### 修改
- 未修改：本轮先审完再集中修复。

### 影响范围
- 已核对模块内调用、直接调用边界及相邻 owner；未提出 API、持久格式或兼容性变更。

### 验证
- 全文及现有测试断言审阅；未新增或运行该文件专项测试，不将静态 PASS 解释为全场景运行证明。

## crates/kanzei-app/src/workspace.rs

### 职责
汇总项目、会话、工作树和后台任务的只读工作区快照。

### 判断
PASS

### 确切问题
- 未发现有实际影响的问题。

### 修改
- 未修改：本轮先审完再集中修复。

### 影响范围
- 已核对模块内调用、直接调用边界及相邻 owner；未提出 API、持久格式或兼容性变更。

### 验证
- 全文及现有测试断言审阅；未新增或运行该文件专项测试，不将静态 PASS 解释为全场景运行证明。

## crates/kanzei-app/src/workspace/tests.rs

### 职责
工作区快照身份、占用和缺失状态的回归测试。

### 判断
PASS

### 确切问题
- 未发现有实际影响的问题。

### 修改
- 未修改：本轮先审完再集中修复。

### 影响范围
- 已核对模块内调用、直接调用边界及相邻 owner；未提出 API、持久格式或兼容性变更。

### 验证
- 全文及现有测试断言审阅；未新增或运行该文件专项测试，不将静态 PASS 解释为全场景运行证明。

## crates/kanzei-app/src/worktree_tests.rs

### 职责
工作树转换、合并、写租约和会话恢复集成回归。

### 判断
PASS

### 确切问题
- 未发现有实际影响的问题。

### 修改
- 未修改：本轮先审完再集中修复。

### 影响范围
- 已核对模块内调用、直接调用边界及相邻 owner；未提出 API、持久格式或兼容性变更。

### 验证
- 全文及现有测试断言审阅；未新增或运行该文件专项测试，不将静态 PASS 解释为全场景运行证明。

## crates/kanzei-app/ui/05-chat-render.js

### 职责
按会话挂载聊天正文、错误卡和工具块。

### 判断
P1

### 确切问题
- AF-R01 错误卡重试读取当前全局 lastRequest（197-206）：A 对话失败后，在 B 发送另一条请求，再回 A 点原错误卡重试。把 B 的提示和附件发进 A，重试不再对应失败请求。

### 修改
- 未修改：本轮先审完再集中修复。
- 后续最小修复：每张错误卡绑定原请求与会话；验证 A→B→A 重试。

### 影响范围
- 01-core.js 按 session 清旧错误；08-compose-runtime.js 仅发送时更新 lastRequest，切回 A 不恢复它。

### 验证
- AF-R01：output/audit-first/root/probe.json#AF-R01；隔离探针执行当前源码，证明现存问题，尚未验证修复。

## crates/kanzei-app/ui/06-activity.js

### 职责
活动详情、文件/diff 结果和运行审计投影。

### 判断
P2

### 确切问题
- AF-R03 分栏 diff 右侧上下文行号取旧版本（566-577）：插入/删除后，同一上下文行 old_line=10、new_line=12，切换为分栏显示。右列仍显示 10，用户按错误行号定位新代码。

### 修改
- 未修改：本轮先审完再集中修复。
- 后续最小修复：每列使用对应版本行号；验证前方插入、删除后的上下文。

### 影响范围
- 统一视图分别消费 old_line/new_line，分栏两列共用 old_line ?? new_line。

### 验证
- AF-R03：output/audit-first/root/probe.json#AF-R03；隔离探针执行当前源码，证明现存问题，尚未验证修复。

## crates/kanzei-app/ui/06-agent-panel.js

### 职责
后台任务面板的状态展示与交互。

### 判断
PASS

### 确切问题
- 未发现有实际影响的问题。

### 修改
- 未修改：本轮先审完再集中修复。

### 影响范围
- 已核对模块内调用、直接调用边界及相邻 owner；未提出 API、持久格式或兼容性变更。

### 验证
- 全文及现有测试断言审阅；未新增或运行该文件专项测试，不将静态 PASS 解释为全场景运行证明。

## crates/kanzei-app/ui/08-compose.js

### 职责
组合输入模块入口和重导出。

### 判断
PASS

### 确切问题
- 未发现有实际影响的问题。

### 修改
- 未修改：本轮先审完再集中修复。

### 影响范围
- 已核对模块内调用、直接调用边界及相邻 owner；未提出 API、持久格式或兼容性变更。

### 验证
- 全文及现有测试断言审阅；未新增或运行该文件专项测试，不将静态 PASS 解释为全场景运行证明。

## crates/kanzei-app/ui/12-decision-console.js

### 职责
工作单元判定、纠正和后续动作界面。

### 判断
PASS

### 确切问题
- 未发现有实际影响的问题。

### 修改
- 未修改：本轮先审完再集中修复。

### 影响范围
- 已核对模块内调用、直接调用边界及相邻 owner；未提出 API、持久格式或兼容性变更。

### 验证
- 全文及现有测试断言审阅；未新增或运行该文件专项测试，不将静态 PASS 解释为全场景运行证明。

## crates/kanzei-app/ui/15-conventions.js

### 职责
用户审阅、保存和丢弃规范建议稿。

### 判断
PASS

### 确切问题
- 未发现有实际影响的问题。

### 修改
- 未修改：本轮先审完再集中修复。

### 影响范围
- 已核对模块内调用、直接调用边界及相邻 owner；未提出 API、持久格式或兼容性变更。

### 验证
- 全文及现有测试断言审阅；未新增或运行该文件专项测试，不将静态 PASS 解释为全场景运行证明。

## crates/kanzei-app/ui/17-files.js

### 职责
文件树、工作树切换、过滤与用途标注交互。

### 判断
PASS

### 确切问题
- 未发现有实际影响的问题。

### 修改
- 未修改：本轮先审完再集中修复。

### 影响范围
- 已核对模块内调用、直接调用边界及相邻 owner；未提出 API、持久格式或兼容性变更。

### 验证
- 全文及现有测试断言审阅；未新增或运行该文件专项测试，不将静态 PASS 解释为全场景运行证明。

## crates/kanzei-app/ui/19-arch.js

### 职责
架构依赖图和设计文档索引展示。

### 判断
P2

### 确切问题
- AF-R08 合法架构索引链接被隐藏（119-139,149-152）：索引使用普通 [example.md](../../../docs/design/example.md)，不把标签包在反引号中。解析器标记已入索引，却不加入分组，未入册兜底也排除，文档从树里消失。

### 修改
- 未修改：本轮先审完再集中修复。
- 后续最小修复：统一条目解析，分组外条目仍展示；验证普通标签、反引号、无分组。

### 影响范围
- architecture.rs 按链接目标校验，不要求标签反引号或特定章节；其测试也接受平铺索引。

### 验证
- AF-R08：output/audit-first/root/probe.json#AF-R08；隔离探针执行当前源码，证明现存问题，尚未验证修复。

## crates/kanzei-app/ui/20-lines.js

### 职责
独立任务、工作树审阅、检查和合并控制。

### 判断
P1

### 确切问题
- AF-R04 重新检查失败沿用旧的合并通过状态（830-918）：工作树检查先成功，再点击重新检查；第二次调用失败/抛异常后点击合并。仍走旧绿色状态，无失败检查确认就调用 worktree_merge；检查运行中也未撤销旧结果。

### 修改
- 未修改：本轮先审完再集中修复。
- 后续最小修复：启动新检查立即使旧结果失效，失败保持失败状态，结束后才能重新判定合并。

### 影响范围
- processes/gate.rs 返回步骤结果；processes/workspace.rs 的 merge 检查工作树空闲与写租约，不保存/复核 gate 结果。

### 验证
- AF-R04：output/audit-first/root/probe.json#AF-R04；隔离探针执行当前源码，证明现存问题，尚未验证修复。

## crates/kanzei-app/ui/21-palette.js

### 职责
命令与文件检索面板。

### 判断
PASS

### 确切问题
- 未发现有实际影响的问题。

### 修改
- 未修改：本轮先审完再集中修复。

### 影响范围
- 已核对模块内调用、直接调用边界及相邻 owner；未提出 API、持久格式或兼容性变更。

### 验证
- 全文及现有测试断言审阅；未新增或运行该文件专项测试，不将静态 PASS 解释为全场景运行证明。

## crates/kanzei-app/ui/24-preview.js

### 职责
网页预览界面、可见性、截图附件和工具结果入口。

### 判断
P1

### 确切问题
- AF-R07 异步截图进入后来切换的对话附件（996-1004）：A 点击截图放进输入框，截图返回前切换到 B。A 截图追加到 B 的附件数组；与普通 FileReader 已有的 target 身份守卫不一致。

### 修改
- 未修改：本轮先审完再集中修复。
- 后续最小修复：截图和批注携带发起会话/附件 owner，迟到结果不得进入新会话。

### 影响范围
- 08-compose-runtime.js:361 的 addPngAttachment 写实时 attachments；普通 addFiles 在 onload 检查 target !== attachments。

### 验证
- AF-R07：output/audit-first/root/probe.json#AF-R07；隔离探针执行当前源码，证明现存问题，尚未验证修复。

## crates/kanzei-app/ui/26-project-conversations.js

### 职责
项目会话清单与新会话入口。

### 判断
PASS

### 确切问题
- 未发现有实际影响的问题。

### 修改
- 未修改：本轮先审完再集中修复。

### 影响范围
- 已核对模块内调用、直接调用边界及相邻 owner；未提出 API、持久格式或兼容性变更。

### 验证
- 全文及现有测试断言审阅；未新增或运行该文件专项测试，不将静态 PASS 解释为全场景运行证明。

## crates/kanzei-app/ui/28-async-workspace.js

### 职责
后台日志、订阅、提问与回复工作台。

### 判断
P2

### 确切问题
- AF-R02 异步工作台旧行使用新 scope（75-82,99-114）：从 A 打开到 B，B 列表尚未返回时点击仍显示的 A 终端操作。请求携带 B 项目/会话和 A 终端 ID，合法操作被后端归属校验拒绝；未证明误停别的进程。

### 修改
- 未修改：本轮先审完再集中修复。
- 后续最小修复：绑定行 owner 或换 scope 时撤掉旧操作，迟到错误也核对 owner。

### 影响范围
- open 先换 scope，refresh 完成前未清旧行；renderLogs/问题回复闭包调用实时 args()。terminal_monitor 有项目守卫。

### 验证
- AF-R02：output/audit-first/root/probe.json#AF-R02；隔离探针执行当前源码，证明现存问题，尚未验证修复。

## crates/kanzei-core/src/runner/drive/parallel_tools.rs

### 职责
工具冲突波次调度与结果顺序恢复。

### 判断
PASS

### 确切问题
- 未发现有实际影响的问题。

### 修改
- 未修改：本轮先审完再集中修复。

### 影响范围
- 已核对模块内调用、直接调用边界及相邻 owner；未提出 API、持久格式或兼容性变更。

### 验证
- 全文及现有测试断言审阅；未新增或运行该文件专项测试，不将静态 PASS 解释为全场景运行证明。

## crates/kanzei-core/src/runner/schema_check.rs

### 职责
子代理结构化结果提取和有限 schema 校验。

### 判断
P1

### 确切问题
- AF-R05 JSON 提取优先内层对象而非外层数组（137-185）：子代理输出 Results: [{"ok":true},{"ok":false}]，期望 schema 为 array。提取为首个 object，合法数组被打回；object schema 还可能错误接受丢失其余数据的子对象。

### 修改
- 未修改：本轮先审完再集中修复。
- 后续最小修复：按最早顶层起始括号解析完整值，测试数组对象嵌套及散文/围栏。

### 影响范围
- runner/subagent.rs:915-918 实际用 extract_json 结果校验并决定重试。文档承诺第一个完整顶层结构。

### 验证
- AF-R05：output/audit-first/root/schema-probe.json；隔离探针执行当前源码，证明现存问题，尚未验证修复。

## crates/kanzei-core/src/runner/tool_exec.rs

### 职责
工具准备、权限、执行、输出归档和批次通知。

### 判断
PASS

### 确切问题
- 未发现有实际影响的问题。

### 修改
- 未修改：本轮先审完再集中修复。

### 影响范围
- 已核对模块内调用、直接调用边界及相邻 owner；未提出 API、持久格式或兼容性变更。

### 验证
- 全文及现有测试断言审阅；未新增或运行该文件专项测试，不将静态 PASS 解释为全场景运行证明。

## crates/kanzei-core/src/runner/tool_failure_telemetry.rs

### 职责
按运行和调用身份记录工具导航失手遥测。

### 判断
P2

### 确切问题
- AF-R06 成功工具正文含 required 仍留下错误的缺参数诊断（89-125）：read 成功读取普通英文或 schema 内容，其中出现 required。诊断 events 仍写入 missing_parameter，产生错误诊断记录。另一任务本轮已修改计数：success 不再增加 failure_count，原关闭失败率污染影响已缓解，不能继续算作当前影响。

### 修改
- 未修改：本轮先审完再集中修复。
- 后续最小修复：优先结构化结果码，文本兜底限定失败结果；保留 grep/glob 空命中的现有专门语义。

### 影响范围
- tool_exec 对成功结果也记录；record_outcome 仍无条件保存非空 class，但并行改动已按 failed outcome 计算 failure_count。

### 验证
- AF-R06：output/audit-first/root/telemetry-probe.txt；隔离探针执行当前源码，证明现存问题，尚未验证修复。

## crates/kanzei-core/src/runner/tool_images.rs

### 职责
工具图像落盘、复用与配额清理。

### 判断
PASS

### 确切问题
- 未发现有实际影响的问题。

### 修改
- 未修改：本轮先审完再集中修复。

### 影响范围
- 已核对模块内调用、直接调用边界及相邻 owner；未提出 API、持久格式或兼容性变更。

### 验证
- 全文及现有测试断言审阅；未新增或运行该文件专项测试，不将静态 PASS 解释为全场景运行证明。

## crates/kanzei-tools/src/arch_diagram.rs

### 职责
从 Cargo 清单生成依赖图和图形源码。

### 判断
PASS

### 确切问题
- 未发现有实际影响的问题。

### 修改
- 未修改：本轮先审完再集中修复。

### 影响范围
- 已核对模块内调用、直接调用边界及相邻 owner；未提出 API、持久格式或兼容性变更。

### 验证
- 全文及现有测试断言审阅；未新增或运行该文件专项测试，不将静态 PASS 解释为全场景运行证明。

## crates/kanzei-tools/src/arch_diagram_lint.rs

### 职责
扫描手写架构图并按既有约定检查语法和链接。

### 判断
PASS

### 确切问题
- 未发现有实际影响的问题。

### 修改
- 未修改：本轮先审完再集中修复。

### 影响范围
- 已核对模块内调用、直接调用边界及相邻 owner；未提出 API、持久格式或兼容性变更。

### 验证
- 全文及现有测试断言审阅；未新增或运行该文件专项测试，不将静态 PASS 解释为全场景运行证明。

## crates/kanzei-tools/src/arch_diagram_tests.rs

### 职责
架构图生成、传递约简、lint 和实际仓库不变量测试。

### 判断
PASS

### 确切问题
- 未发现有实际影响的问题。

### 修改
- 未修改：本轮先审完再集中修复。

### 影响范围
- 已核对模块内调用、直接调用边界及相邻 owner；未提出 API、持久格式或兼容性变更。

### 验证
- 全文及现有测试断言审阅；未新增或运行该文件专项测试，不将静态 PASS 解释为全场景运行证明。

## crates/kanzei-tools/src/architecture.rs

### 职责
带 CAS 和增量验证的架构索引专用写通道。

### 判断
P2

### 确切问题
- AF-R11 同一索引目标的点段别名逃过重复检查（528-566）：同一 example.md 分别以 ../../../docs/design/example.md 和 ../../../docs/design/./example.md 入索引。校验显示零问题，允许同一实体重复入册；compliant 却归一为一份，内部身份口径不一致。

### 修改
- 未修改：本轮先审完再集中修复。
- 后续最小修复：重复检查和版本比较共用规范化目标身份，保留原链接用于展示。

### 影响范围
- seen 用原始 bare；dropped_entries/compliant 用 project_rel 规范身份。探针实际验证 2 链接/0问题/1规范条目。

### 验证
- AF-R11：output/audit-first/root/contracts-probe.txt；隔离探针执行当前源码，证明现存问题，尚未验证修复。

## crates/kanzei-tools/src/conventions.rs

### 职责
规范读取、创建、建议稿和局部替换工具。

### 判断
P1

### 确切问题
- AF-R10 已解码 JSON 字符串被再次解码转义（175-179,294-304）：规范 patch 替换内容含 Windows 路径 C:\new\rules 或字面量 \n。路径中的 \n、\r 被改成换行，实际保存内容错误；旧文本也可能无法匹配而拒绝合法 patch。

### 修改
- 未修改：本轮先审完再集中修复。
- 后续最小修复：先按真实字面文本处理；确需兼容时只能明确、无歧义地回退，不改变新文本含义。

### 影响范围
- serde_json 已解码输入；decode_escaped_newlines 无条件 replace，再直接传给 CAS 写入。旧注释为迁就模型双重转义而加兼容。

### 验证
- AF-R10：output/audit-first/root/contracts-probe.txt；隔离探针执行当前源码，证明现存问题，尚未验证修复。

## crates/kanzei-tools/src/conventions/drafts.rs

### 职责
规范文件、待审稿和用户 CAS 保存的单一写语义。

### 判断
PASS

### 确切问题
- 未发现有实际影响的问题。

### 修改
- 未修改：本轮先审完再集中修复。

### 影响范围
- 已核对模块内调用、直接调用边界及相邻 owner；未提出 API、持久格式或兼容性变更。

### 验证
- 全文及现有测试断言审阅；未新增或运行该文件专项测试，不将静态 PASS 解释为全场景运行证明。

# Module Summary

## 已修复
- P0: 0
- P1: 0（确认待修 6）
- P2: 0（确认待修 5）

## PASS 文件
- crates/kanzei-app/src/commands/os_open.rs
- crates/kanzei-app/src/commands/run/tool_process.rs
- crates/kanzei-app/src/deliveries.rs
- crates/kanzei-app/src/files_view.rs
- crates/kanzei-app/src/harness_ext.rs
- crates/kanzei-app/src/preview/cdp.rs
- crates/kanzei-app/src/preview/commands.rs
- crates/kanzei-app/src/preview/console.rs
- crates/kanzei-app/src/preview/host.rs
- crates/kanzei-app/src/preview/mod.rs
- crates/kanzei-app/src/preview/pane.rs
- crates/kanzei-app/src/preview/pure_tests.rs
- crates/kanzei-app/src/processes/gate.rs
- crates/kanzei-app/src/processes/workspace.rs
- crates/kanzei-app/src/screenshot.rs
- crates/kanzei-app/src/verification_monitor.rs
- crates/kanzei-app/src/workspace.rs
- crates/kanzei-app/src/workspace/tests.rs
- crates/kanzei-app/src/worktree_tests.rs
- crates/kanzei-app/ui/06-agent-panel.js
- crates/kanzei-app/ui/08-compose.js
- crates/kanzei-app/ui/12-decision-console.js
- crates/kanzei-app/ui/15-conventions.js
- crates/kanzei-app/ui/17-files.js
- crates/kanzei-app/ui/21-palette.js
- crates/kanzei-app/ui/26-project-conversations.js
- crates/kanzei-core/src/runner/drive/parallel_tools.rs
- crates/kanzei-core/src/runner/tool_exec.rs
- crates/kanzei-core/src/runner/tool_images.rs
- crates/kanzei-tools/src/arch_diagram.rs
- crates/kanzei-tools/src/arch_diagram_lint.rs
- crates/kanzei-tools/src/arch_diagram_tests.rs
- crates/kanzei-tools/src/conventions/drafts.rs

## 仍需人工判断
- 无产品/架构决策阻塞；以上均可按现有合同修复。

## 依赖影响
- 尚未修改 cross-module contract；需统一错误请求/截图 owner、browser 状态 owner 和规范化索引身份。

## 剩余风险
- 面板锁等待期间换线 AF-R09 只有静态交错证据，修复阶段需补原生或带调度控制的集成回归。
- 其余受控探针不代替桌面端 E2E；后续修复需运行相关 caller/check/test。
