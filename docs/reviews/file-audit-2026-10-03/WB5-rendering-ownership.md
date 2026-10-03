# WB5 翻译、图引擎、工具摘要与结构化展示

日期：2026-10-04。基线 `26acb7f0`，保留 WB1–WB4 修复。本包全文审查 4 文件，另修改 18-startup 的一处调用合同；共修复 3 个 P1、5 个 P2。WB5 编号仅为报告定位。

## 依赖地图

```text
app.json 偏好 → core.uiPrefsLoad / layout → 18-startup → 02-i18n → UI 显示
用户语言选择 → 02-i18n → settings/layout/core 保存队列 → app.json
04-status-words → 02-i18n
主题 token / Mermaid 引擎 → 04-diagram（串行队列、缓存、图实例）→ 04-markdown
Rust 工具输出 / outcome → 04-structured-parse → 05-tool-summary
02-i18n + 04-markdown + 04-structured-parse + 05-tool-summary 的 root helpers
  → 04-structured → chat/activity/events、工具结果、权限与查看器
```

本轮明确实际依赖方向：`04-structured` 依赖 `05-tool-summary` 的 root helpers，因此先稳定摘要层，再整合结构化展示。词典与业务 UI 已有 deferred 初始化环，未因外观拆分。内容与运行状态仍由后端拥有；摘要不执行工具。图实例拥有自己的请求代次、DOM、观察器和拖动监听；共享引擎拥有串行队列和缓存。JSON 展示预算与分页位置属于单棵树，不能把“请求显示多少”当作“已显示多少”。

历史依据：源内 UX-119/D-404（语言即时保存、localStorage 为首屏缓存）、D-135/D-202/R-140（仅在渲染点翻译、不改用户原文），以及 [图渲染设计](../../design/architecture_diagrams.md)、UI-0926 #6/#10 的统一摘要与结构化展示合同。修改前已搜索公开 API 全部调用点；边界外功能只核对共享接口。

## crates/kanzei-app/ui/02-i18n.js

### 职责
管理翻译词典、语言选择及页面显示同步，不直接拥有持久化偏好文件。

### 判断
P1

### 确切问题
- **WB5-01 / P1**：启动读取慢返回时覆盖用户刚选的语言。现代偏好路径造成显示/缓存与 app.json 分叉；旧版迁移还会把旧值再次保存，覆盖新选择。

### 修改
- 记录用户已主动选择；后续启动同步返回 false，不再接管语言。未冲突的合法同步返回 true。
- 唯一迁移 caller 在结果被接受后才保存，避免 UI 拒绝而持久化继续写旧值。

### 影响范围
- 18-startup 的两条初始化路径；16-settings 回填同步读取最新 localStorage，无 await 间隙，不使用旧 settings 响应覆盖语言。
- `syncLanguagePreferenceFromSettings` 增加 boolean 返回合同，全部 caller 已核对；无存储键、词典或格式变化。

### 验证
- 真实 Edge、真实初始化及保存队列，延迟 IPC 响应：26 PASS；精确旧 02/18 源码为 17 PASS / 9 FAIL。
- 验证选择框、DOM、缓存、翻译和实际 `ui_prefs_set` 顺序；正常加载与正常旧配置迁移继续生效。

## crates/kanzei-app/ui/04-diagram.js

### 职责
串行调用 Mermaid、缓存结果，并管理图实例的渲染、交互与生命周期。

### 判断
P1

### 确切问题
- **WB5-02 / P1**：图页切换后，已移除实例的迟到结果仍调用 onRendered，覆盖当前图页脚；destroy 后也可发布旧结果，并未释放观察器。
- **WB5-03 / P2**：新源码渲染失败时，错误卡出现但旧 SVG、节点/边事实和拖动状态仍保留，图与当前源码不一致。
- **WB5-04 / P2**：排队中的暗色渲染在切到亮色后才读取 token，却以暗色 key 缓存，后续命中得到错误配色。实际对照将白色标签错误缓存为黑色。

### 修改
- 按实例生命周期和渲染代次接受结果；已脱离 DOM 的异步实例销毁，释放 ResizeObserver 和活动拖动监听。主题及批量清理统一走 destroy。
- 失败时清空旧图和派生图事实，保留当前失败源码与错误提示。
- 入队时捕获主题 token，使排队任务与缓存 key 对应同一次请求。

### 影响范围
- 19-arch 实际 tab/footer、15-views-misc 放大查看器、04-markdown 流式图块；公共签名、Mermaid 版本及图源码格式不变。
- 已销毁或被替换实例不能再发布结果；新的请求继续复用共享引擎队列和缓存。

### 验证
- 10 PASS；精确旧模块为 5 PASS / 5 FAIL。实际 19-arch tab 切换验证当前 footer，原生 ResizeObserver 验证释放；可控引擎延迟验证旧回调、缓存主题和失败恢复。
- 另用真实 Mermaid 对两张生产架构图运行暗/亮渲染、尺寸、映射、安全及坏样例自检，0 警告。可控引擎测试与真实引擎测试分别记录。

## crates/kanzei-app/ui/05-tool-summary.js

### 职责
将工具参数、结果和终态事实生成统一摘要，供实时、历史及活动列表使用。

### 判断
P1

### 确切问题
- **WB5-05 / P1**：Rust 正常返回“项目没有独立 Git 仓库”，摘要却按 status/diff/log 动作显示改动数、空 diff 或提交数，输出错误结果。
- **WB5-06 / P2**：按前缀排除所有 `+++`/`---` 行，正文以 `++`/`--` 开头的真实增删漏计；展开内容正确但摘要统计错误。

### 修改
- 动作统计前识别无仓库事实，复用既有翻译，悬停保留原因；不把正常观察改成工具失败。
- 删除重复计数，复用已稳定的 `parseUnifiedDiff`，与差异查看共用文件头/hunk 规则。

### 影响范围
- 05-chat-render、06-activity、07-events；04-structured/05-subagents 只使用 root helpers。边界外记忆页面仅核对共享摘要接口。
- 读取 `kanzei-tools/src/git/tool.rs` 的 RepoState 输出和 harness ToolOutcome 合同；无公共签名、后端执行状态或持久化变化。

### 验证
- 真实 Edge ESM/DOM 25 PASS；精确旧摘要模块 11 PASS / 14 FAIL。
- 无仓库/上级仓库 × 三种动作；真实 Git 生成中文路径 diff；普通、二进制、空 diff；干净/脏状态、log、noop、needs_correction、命令失败与晚到耗时控制通过。

## crates/kanzei-app/ui/04-structured.js

### 职责
将结构化事实安全展示为 JSON 树、表格、路径、参数、权限及错误详情。

### 判断
P2

### 确切问题
- **WB5-07 / P2**：JSON 分页按请求页尾推进，而嵌套节点会提前耗尽共享预算；实际未显示的行被跳过，按钮消失，动态触顶还缺提示。默认 200 行 × 10 字段可复现，原始 JSON 复制仍可兜底。
- **WB5-08 / P2**：异构 JSON 表格使用 `column in row`，缺失 constructor/toString 字段的行被错误显示成 Object 原型函数。

### 修改
- 分页位置按实际追加行推进，受限节点保留继续入口，触顶提示在动态展开后同步且只添加一次；不提高初始预算，继续沿用用户点击增加预算的机制。
- 表格只读取 Object.hasOwn 的自有字段。

### 影响范围
- 所有工具结果 JSON 展开与表格显示；导航、权限、参数、错误 API 不变。原始内容与复制出口保留。

### 验证
- 真实 Edge 20 PASS；精确旧模块 13 PASS / 7 FAIL。
- 默认预算和初始小预算均可继续到全部字段，既有标量分页通过；危险 HTML 惰性、edit 原参数折叠、权限文本和 lazy 单次构造控制通过。

## Caller 切片：crates/kanzei-app/ui/18-startup.js

### 职责
本次仅核对启动语言恢复和旧配置迁移调用。

### 判断
P1

### 确切问题
- WB5-01 同一根因：迁移无条件保存旧语言，与 UI 是否接受脱节。

### 修改
- 仅 `syncLanguagePreferenceFromSettings` 接受时持久化迁移结果。

### 影响范围
- 只改变冲突迁移路径；不重复计算问题、不增加全文覆盖。

### 验证
- 包含在语言 26 项实际初始化回归中，正常迁移仍保存一次，冲突时不追加旧值。

## 整合验证与进度

- 新增 **81 项**：语言 26、图实例 10、工具摘要 25、结构化展示 20；全部接入 UI runtime gate，重复运行不重复计数。
- ESLint、既有 Markdown smoke、翻译静态检查（2926 key / 651 HTML / 56 合同）通过。
- 完整 UI gate：exit 0；90 个 UI 模块、3606 次初始化 invoke、10 个主视图、0 运行时错误，包含既有浏览器套件及深浅主题 Harness 操作。
- UI lint smoke：exit 0；186 文件、1646 导出 ESM guard，以及附带弹层/布局/背景浏览器检查通过。此前 WB1–WB4 的 19 个主审文件源码指纹保持一致。
- 前后对照及日志：`output/audit-WB5/{i18n,diagram,summary,structured}/`；图真引擎证据 `diagram/real-engine.log`，整合证据 `runtime.log`。
- 本包新增全文 4 文件，18-startup 只记合同切片；完成后全仓历史全文 159/514，四块 84/268 已完成或可复用、184 待处理，工作台 36/51。未提交、未发版。
- 下一包 WB6：`src/attachments.rs` → `06-deliveries.js`；`06-side-policy.js` 和 `22-activity-state.js` 先于活动控制器稳定。

# Module Summary

## 已修复
- P0：0。
- P1：3（WB5-01、02、05）。
- P2：5（WB5-03、04、06、07、08）。

## PASS 文件
- 无；4 个主审文件均有确证问题。

## 仍需人工判断
- 无。

## 依赖影响
- 语言初始化同步返回接受结果，迁移 caller 消费该结果；摘要复用共享 diff parser。其他公共 API、IPC、存储格式和执行语义不变。

## 剩余风险
- 本轮未改 Rust；未重跑 Rust 全工作区或原生 WebView2/真实 app.json 联调。证据为生产 ESM、真实 Edge、原生观察器、实际 Git、真实 Mermaid及隔离 IPC。
- C7 原有三个 P1 仍在第二块排队，本轮不宣称已修。
