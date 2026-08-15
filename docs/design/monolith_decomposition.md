# 巨石文件拆解(app/main.rs · ui/main.js · core/runner.rs · core/store.rs)

- 状态:设计基线(2026-08-09 用户定调「架构 entropy 增速开始追上 feature velocity,拆解优先级最高」)
- 日期:2026-08-09
- 关联需求:R-153(app/main.rs)、R-154(ui/main.js)、R-155(runner.rs+store.rs);前置 R-152(CI 安全网)
- 关联缺陷:无
- 关联决策:A-008
- **后续:本文覆盖的是第一轮(2026-08-09)。第二轮拆解对象(`app/run.rs`、`memory/store.rs`、`app/processes.rs`)见 [monolith_decomposition_round2.md](monolith_decomposition_round2.md)——本文拆出的 `run.rs` 六天后重新长成 2885 行生产码,那正是第二轮要解决的分层问题。**

> **行号基准:commit c339b58(2026-08-09)。** 执行时一律以**符号名**定位,行号只作导航参考;若执行时行号明显对不上,说明有并发提交插入,先 `git log` 核对再动手。

## 背景与问题

| 文件 | 行数 | 问题 |
| --- | --- | --- |
| `crates/kanzei-app/src/main.rs` | 6413 | 75 个 tauri command + 全部辅助 + 818 行测试挤在一个文件 |
| `crates/kanzei-app/ui/main.js` | 7020 | 全部前端(18 个功能域)单文件 |
| `crates/kanzei-core/src/runner.rs` | 3240 | 模型循环/压缩/调度/子代理/指标混居,`run_once_with_parts` 单函数 778 行 |
| `crates/kanzei-core/src/store.rs` | 1972 | 7 张表全部仓储方法 + 29 个测试单文件 |

自举项目的巨石危害大于普通项目:agent 检索粒度变差、context 被无关符号吃掉、patch locality 下降 → 继续向大文件追加 → 自增强。入口文件已成 attractor(files_view.rs 文件头自己都这么写)。

## 目标与非目标

- 目标:文件级模块化;**零行为变更**;**外部 API 面零变更**(`kanzei_core::X` 顶层再导出路径全部保持);每批独立编译、全量测试可过、可单独提交回滚。
- 非目标:不引 ES modules/打包器/前端框架(A-008);不拆 `run_task`(695 行)与 `run_once_with_parts`(778 行)**内部**(只整体搬迁,内部拆分另立条目);不重构 `migrate()`;不删零调用 pub 方法(拆完另行清理);不顺手改任何行为/文案/格式。

## 执行纪律(三个条目共用)

1. **每批 = 一次提交**,提交前做**定向验证**(节奏依 conventions §1.4/A-010,2026-08-09 用户定调效率优先):R-153 → `cargo test -p kanzei-app`;R-155 → `cargo test -p kanzei-core` + `cargo check -p kanzei -p kanzei-app -p kanzei-tools`(顺带完成外部 API 面断言);R-154(ui/)→ `node --check`(遍历 ui/*.js)+ 四条冒烟(秒级,每批保持)。**全量 `cargo test --workspace` 只在条目关闭前与发版前跑**,不挂在每批提交上。
2. 每批提交后顺手 push(带代理),CI 异步全量兜底;CI 红了先修复再开下一批。**外部 API 面零变更**以纪律 1 的三 crate `cargo check` 为每批断言(R-155 硬验收;R-153 拆的就是 kanzei-app 自身,断言只对另外两个 crate 成立)。
3. **拆解批与任何其他源码条目不得并发**(并发自举提交会与大搬迁冲突);动手前 `git status` + 核对最新提交。
4. 搬迁 = 剪切粘贴 + 最小可见性调整(`pub(crate)`/`pub(super)`),diff 里只允许 move + use + 可见性,出现逻辑 diff 即回退重来。

---

## A. kanzei-app/src/main.rs(R-153)

### 既有先例:files_view.rs 模式(照抄)

- 文件头 `//!` 写清独立原因;command 函数加 `pub`(`#[tauri::command]` 宏展开需要);main.rs 只 `mod xxx;` + invoke_handler 里写全路径 `xxx::cmd_name`,**不写 `use xxx::*`**;模块自带 `#[cfg(test)] mod tests`;低耦合模块对 main.rs 零依赖。

### 目标模块划分(吸收行号段为 c339b58 基准)

| 文件 | 吸收行号 | 内容 |
| --- | --- | --- |
| `main.rs`(瘦身后 ≤300 行) | 1–31, 1631–1753 | mod 声明、`main()`、setup、invoke_handler |
| `state.rs` | 34–39, 75–129, 131–434 | `AppState`/`SessionRuntime`/`LiveRun`/`PendingAsk`/`ProcessHandle`、三个 UI_PROBE static、全部跨域辅助(`normalized_project_root`/`with_session_id`/`process_session_id`/`runtime_for` 等)、`ui_probe_result` command、`hidden_command` |
| `update.rs` | 436–810, 4026–4168, 测试 6207–6255 | 启动接棒/安装校验/更新检查,`update_check`/`update_install` |
| `fast_model.rs` | 2117–2321 | ollama 六辅助 + `fast_model_status`/`fast_model_setup` |
| `agent_container.rs` | 4545–4613 | 三 command,零共享依赖 |
| `mobile.rs` | 260–268, 4384–4543 | 移动服务 |
| `memory.rs` | 3604–4021 | 13 个 memory command;`consolidate_memory_inbox` 提 `pub(crate)`(run_task 在调) |
| `prefs.rs` | 1757–1790 | `AppPrefs` 读写 |
| `projects.rs` | 2019–2113, 2325–2468, 2515–2542, 2682–2711 | 12 个项目 command |
| `processes.rs` | 232–258, 279–286, 1792–2017 | 8 个进程/工作树 command |
| `settings.rs` | 2712–3179, 4171–4230, 4812–4918 | 9 个设置/供应商 command |
| `docs.rs` | 2543–2680, 4233–4369, 4616–4633 | 8 个文档/追踪器 command |
| `conversation.rs` | 4921–5101, 5453–5508 | 5 个历史 command + `recover_messages*` |
| `harness_ext.rs` | 3184–3336 | UiDom/UiConsole/UiStyle 工具、`FrontendToolsComponent`/`QuickCaptureComponent` |
| `subagents.rs` | 3338–3602 | `defect_review`/`quick_req` |
| `run.rs` | 41–73, 4637–4809, 5111–5451, 5510–6204, 测试 6257–6306 | 运行主链路 + `run_task`(整体搬迁) |

### 批次(每批独立编译提交)

- **批0**:把 `mod update_tests`(812–1629,818 行)按域切成 5 个 `#[cfg(test)] mod`(update/state/process/conversation/权限),各自 `use super::{…}` 收窄。零生产代码变更,解锁全部后续批(否则每批都返工它)。
- **批1**:`agent_container.rs` + `fast_model.rs`(零 AppState 依赖叶子,练手验证「command 加 pub + 全路径注册」)。
- **批2**:`update.rs`(main 保留 `update::startup_update()` → `sync_bundled_cli()` → `cleanup_orphan_webviews()` 三行**及其顺序**——启动接棒/D-175/D-171 硬约束)。
- **批3**:`memory.rs`。批4:`state.rs`(枢纽,5~10 的地基)。批5:`prefs.rs`+`projects.rs`。批6:`processes.rs`+`mobile.rs`。批7:`settings.rs`+`docs.rs`。批8:`conversation.rs`。批9:`harness_ext.rs`+`subagents.rs`。批10:`run.rs`(最难,最后;顺手按域重排 invoke_handler 并加分组注释)。
- 批11(可选,**另立条目再做**):`run_task` 内部拆装配线/事件循环/收尾三段。

### 危险点

1. `main()` 开头三调用顺序是硬约束(见批2)。
2. `UI_PROBE_EMIT`(OnceLock,setup 单次写入)/`UI_PROBE_SEQ`/`UI_PROBES` 三 static 与 `ui_probe`/`ui_probe_result` 必须同住 state.rs,`pub(crate)`。
3. `AppState.ask_seq` 是共享 `Arc<AtomicU64>`,不得在任何模块新建。
4. `#[cfg(windows)]` 6 处分散:`process_alive` 成对定义(699/708)**必须成对搬**;ollama 域 2140/2227/2254 三处内联 creation_flags 没走 `hidden_command`,拆 fast_model.rs 时漏 cfg 会断非 Windows 编译。
5. `assembly_tests` 守的是 run_task 里 FrontendToolsComponent 注册与提示词追加的双写点(D-195/D-190),这两处必须留在同一函数。
6. `use super::*` 的测试模块(assembly/settings)拆分后只在测试编译时报错,`cargo build` 看不见——每批的 `cargo test -p kanzei-app`(会编译测试)不可省,不能用 `cargo build -p` 替代。

---

## B. kanzei-app/ui/main.js(R-154)

### 机制事实(方案依据,已核实)

- index.html 仅 `<script src="main.js" defer>` 一处引用;**无 inline 事件属性**;`tauri.conf.json` frontendDist 是整目录,新增文件自动打包。
- classic script 顶层 `let/const` 进**全局词法环境**(跨脚本共享、受 TDZ 约束),顶层 `function` 上全局对象——多文件按序加载与单文件语义一致,唯一约束是顺序。
- **四个冒烟脚本全部硬编码单文件路径**:i18n/a11y 纯静态正则(拼接串即可);markdown 用 marker 切片(要求 04/05 两段相邻);runtime 是 `vm.createContext` + 单次 `runInContext` 真实执行,并对源码做两处正则注入(6759 启动 IIFE、843 reportPersistentError)+ 尾部追加 `__kzTest` hook。
- **拼接后一次 vm 执行会掩盖真 bug**(函数声明被提升到整串顶部,浏览器多脚本下的 ReferenceError 在 vm 里跑通)→ 必须**逐文件 runInContext**(同 context 多次调用与浏览器多 `<script>` 语义完全一致,含 TDZ)。
- **现存唯一前向引用硬风险**:L3244 `processProfileUi` 初始化调用 L3254 的 `readJson`,靠同脚本函数提升成立——拆分时 `readJson`/`writeJson` 必须上提到 01。
- 启动 IIFE(6729–6763)必须是最后一个脚本。

### 目标文件(全部在 ui/,数字前缀=加载顺序,index.html 依序 18 个 `<script ... defer>`)

| # | 文件 | 吸收行号 | 依赖要点 |
| --- | --- | --- | --- |
| 01 | `01-core.js` | 1–81 **+ 3252–3268(readJson/writeJson 上提)** | 无 |
| 02 | `02-i18n.js` | 82–716 | 01 |
| 03 | `03-shell.js` | 717–1080 | 01 02 |
| 04 | `04-markdown.js` | 1081–1218 | 01;**必须与 05 相邻**(markdown 冒烟切片) |
| 05 | `05-chat-render.js` | 1219–1508 | 01–04 |
| 06 | `06-activity.js` | 1509–1999 | 01 03 04 |
| 07 | `07-events.js` | 2000–2555 | 01 03 05 06 |
| 08 | `08-compose.js` | 2556–3496(剔除 3252–3268) | 01 03 05 |
| 09 | `09-sessions.js` | 3497–4059 | 01 03 08 |
| 10 | `10-docs-core.js` | 4060–4298 | 01 03 |
| 11 | `11-docs-list.js` | 4299–4824(renderDocList) | 10 |
| 12 | `12-docs-pages.js` | 4825–5024 | 10 11 |
| 13 | `13-memory.js` | 5025–5565 | 01 03 04 |
| 14 | `14-docs-actions.js` | 5566–5855 | 10 11 12 |
| 15 | `15-views-misc.js` | 5856–6128 | 04 05 06 |
| 16 | `16-settings.js` | 6129–6699 | 01 03 08 |
| 17 | `17-files.js` | 6700–6728 + 6765–7020 | 01 02 03 |
| 18 | `18-startup.js` | 6729–6763(启动 IIFE) | **全部,必须最后** |

### 批次

- **B0(使能批,main.js 一字不动)**:改四个冒烟脚本——从 index.html 解析 `<script src>` 清单按序读入 `sources[]`;runtime 冒烟逐文件 `vm.runInContext`,`__kzTest` hook 单独最后执行,两处探针注入改为对每个文件各跑 replace、**累计命中 ≥2 才算注入成功**;静态断言(includes/indexOf 切片)一律用 `sources.join("\n")`。同步把 `docs/design/deep_parallel_dev.md:283` 的 `node --check .../main.js` 改成遍历。**此批四冒烟必须仍绿**(单文件是清单的退化情形,纯机制改造零行为变化)。
- **B1**:切出 18-startup + 17-files(files 段原在 IIFE 之后,调换为 files 在前、IIFE 锁死末位——IIFE 步骤清单不含 refreshFiles,调换等价)。首批验证多脚本机制。
- **B2**:16-settings、15-views-misc。**B3**:14-docs-actions、13-memory。**B4**:12/11/10 docs 三件。**B5**:09-sessions。**B6**:08-compose,**同批把 readJson/writeJson 上提**(否则 L3244 前向引用当场炸)。**B7**:07-events、06-activity。**B8**:05-chat-render、04-markdown(相邻落位)。**B9**:03-shell、02-i18n,残余(1–81+readJson/writeJson)改名 01-core.js,main.js 消失。收尾。
- 从尾部往前切:每批只影响文件尾部行号,前面段落行号稳定,diff 最干净。

### 每批验证

`node --check` 遍历 ui/*.js + 四条冒烟。style.css 零改动;index.html 只动 script 标签区。

---

## C. kanzei-core runner.rs + store.rs(R-155)

### 外部 API 面(已 Grep 核实)

外部三 crate **零处**使用 `kanzei_core::runner::`/`::store::` 模块路径,全走 `kanzei_core::X` 顶层再导出 → `runner/mod.rs`、`store/mod.rs` 内 `pub use` 平铺子模块符号,**lib.rs 与外部三 crate 一行不改**,作为每批验收断言。

### runner/ 划分与批次(B1→B8 顺序即依赖顺序)

| 批 | 新文件 | 吸收行号 | 要点 |
| --- | --- | --- | --- |
| B1 | `runner/event.rs` | 414–513, 1379–1403, 2541–2564 | RunEvent/RunSummary/Ask*/preview,零内部依赖 |
| B2 | `runner/metrics.rs` | 515–661, 834–1035 + 对应测试 | `is_git_query` 提 `pub(crate)`(双归属:指标+冗余) |
| B3 | `runner/redundancy.rs` | 663–832 + 对应测试 | 此批后 `failure_tests` 整体消失;共享测试辅助(call/result/bash 等)建 `#[cfg(test)] pub(crate) mod testutil` |
| B4 | `runner/context.rs` | 63–118, 225–369, 2523–2529 | `is_text_user_message` 三归属,归此处 `pub(super)` |
| B5 | `runner/compaction.rs` | 120–223, 2413–2522 | 依赖 B2(`dropped_trace` 调 summarize_tools/failures)+ B4 |
| B6 | `runner/tool_exec.rs` | 2202–2300 + 对应测试 | `PreparedToolCall` **六个字段**提 `pub(super)`(测试按字面量构造) |
| B7 | `runner/subagent.rs` | 371–412, 2301–2411 | 经 `super::run_once` 调用 |
| B8 | `runner/drive.rs` | 1405–2200, 2531–2539 | `run_once_with_parts` 整体搬迁,**不动内部** |

`runner/mod.rs` 留常量(`MAX_FUTILE_COMPACTIONS` 对子模块 `pub(super)`,测试断言其字面值)+ `RunnerConfig` + `pub use` 平铺。测试**随代码分域下沉**,不建统一 tests.rs(否则 15 个私有项被迫集体 `pub(crate)`,封装拆没)。

### store/ 划分与批次

S1 拆壳:`store.rs`→`store/mod.rs` 原样改名,`connection`/`path` 字段 `pub(crate)` 化(注释「仅限 store::* 子模块使用」),**单独一批**。S2 `episodes.rs` → S3 `notifications.rs` → S4 `events.rs`(`append_event_tx` 提 `pub(crate)`)→ S5 `inbox.rs`(`Delivery::as_str` 提 `pub(super)`;`finalize_interrupt` 跨域事务放这里)→ S6 `session.rs` → S7 `schema.rs`(`migrate` 188 行**原样搬**,不重构;注意其中 `'schema_version','7'` 是硬编码字面量,**不许顺手改成常量**)→ S8 测试分域(直操 `store.connection` 的迁移测试 1311–1579 全部跟 schema.rs 走)。

### 危险点

1. **`run_once` 必须保持 `Pin<Box<dyn Future>>` 签名**——它与 `run_subagent` 递归,boxed 是无限类型的断点;拆到两个文件后改成 `async fn` 立刻 E0072。两处都加注释锁死。
2. `run_once_with_parts` 有 12 个跨段可变局部 + `on_event`/`ask` 双 `&mut dyn FnMut` 跨 await 重借用——**B8 只整体搬迁,任何抽函数动作都不在本条目做**。
3. **`calls[i]`↔`results[i]` 下标对齐不变式**跨 tool_exec/redundancy/drive 三文件后无人可见:给 `RedundancyWatch::note_step` 加 `debug_assert_eq!(calls.len(), results.len())` + 三处注释(这是本条目唯一允许的非 move 改动)。
4. `LlmEvent` 大 match 带 `_` 兜底;`stream_error` 三臂 guard **臂序有语义**(overflow 必须在 Transport 前);`Gate` 匹配 UserDeclined 臂内含 return——一律原样搬,禁止重排。
5. `defect_known_path_hint` 有文件 IO(读 `.kanzei/project/defects.md`),其测试依赖真实目录布局,搬迁时相对路径一起核。
6. store 的 `unchecked_transaction()` 绕过 `&mut self`,嵌套事务 rustc 不查——`events.rs` 顶部写明「已在事务内不得再调自开 tx 的方法」。
7. `TaskTrace` 是 `RunEvent::TaskProgress` 字段类型,降 `pub(crate)` 会触发 private_interfaces lint——保持 pub 但不再导出。`RunMetrics` 是 `summarize_metrics` 返回类型,必须保持 pub 且再导出。
8. 零外部调用的 pub 方法(`append_notification`/`has_pending`/`promote_steers`/`backup_path` 等 10 个)**保留不删**;`backup_path`/`legacy_inputs_recovered` 是 D-180 事后核对入口。

---

## 技术选型与取舍

| 选择 | 备选 | 理由 |
| --- | --- | --- |
| 前端有序 classic script | ES modules | ES modules 要重写 runtime 冒烟(experimental vm modules 或全局注入+原生 import)、显式化数百处跨文件引用、且模块不建全局绑定会改变语义面;classic 多脚本与现冒烟机制逐文件等价可验证。详见 A-008,日后要上 ES modules 须新开设计文档 |
| 测试随域下沉 | 统一大 tests.rs | 后者迫使全部私有项 `pub(crate)`,封装白拆 |
| 从尾部往前切(前端) | 从头往前切 | 尾切时未动段落行号稳定,与行号地图偏差最小 |
| run_task/run_once_with_parts 只搬不拆 | 同批内部重构 | 借用检查风险(双 &mut dyn FnMut 跨 await)与搬迁风险必须分离;内部拆分另立条目 |

## 实施边界与调用方

- R-153/R-155 触碰 `crates/kanzei-app/src/`、`crates/kanzei-core/src/`;R-154 触碰 `crates/kanzei-app/ui/*.js`、`index.html` script 区、四个冒烟脚本、deep_parallel_dev.md 一行。
- 全程零行为变更、零外部契约变更;发布节奏不受影响(任意批次间可发版)。

## F. Desktop 与 CLI 共用 RunService(R-256)

来源:第二轮巨石扫描 R4(收益最大、风险最大,排在三巨头之后)。目的:两端各写一遍
编排,每加一个运行期能力要改两处,且只有桌面端被真实验证。合并前**先做只读对照**
——装配步骤逐项比对,漂移的先对齐再合并,不在合并动作里顺手改行为。

对照基准:`crates/kanzei/src/main.rs`(生产码 1378,核心 `run_cli` L335-1043,713 行)
vs 桌面端 `crates/kanzei-app/src/run/`(R-253 后 assembly/coordinator/execution/
persistence/events 五域)。

### 装配步骤对照表(2026-08-16 实测)

| # | 装配/运行步骤 | CLI(run_cli) | 桌面(run/) | 判定 |
|---|---|---|---|---|
| 1 | 取根 | `main_project_root(explicit_main_root)` L361 | run_prompt 入口解析后显式传 main_root(R-141) | **有意**(CLI 自解析;桌面 IPC 入口一次解析) |
| 2 | 配置加载+告警 | `load_with_warnings_at_root` + eprintln L363 | 同函数 + `report_config_warnings`(UI) | **漂移**(加载逻辑重复,仅输出端不同) |
| 3 | profile 解析 | KANZEI_PROFILE env / readonly / default L371 | run_task 参数 profile | **有意**(env vs IPC 参数) |
| 4 | ResolveCtx 构造 | L377 四字段 | assembly.rs 同结构 | **重复** |
| 5 | harness 装配 | Base/Dev/Research/**Readonly**/Markdown/Config L385 | build_run_harness:Base/Dev/Research/**FrontendTools/Markdown/Config/TrackerWritePolicy**(+Collaboration) | **漂移**(基础组件重复;两端各有独有组件) |
| 6 | agent 选择 | `select_agent(KANZEI_AGENT env)` L395 | `select_agent(agent_name 参数)` | **有意**(env vs 参数) |
| 7 | dev 提示注入 | `resolved_control_prompt` L399 | `append_dev_guidance` + `resolve_work_decision` | **漂移**(两套注入逻辑) |
| 8 | 模型解析 | `resolve_model_chain(KANZEI_MODEL,None)` L411 | `resolve_model_chain(model_override,None)` | **漂移**(同函数,参数源不同) |
| 9 | proxy 解析 | KANZEI_PROXY env 覆盖 L418 | `resolve_proxy(&config)` | **漂移**(CLI 有 env 覆盖,桌面无) |
| 10 | route/client | `build_route` + `LlmClient::new` L427 | 相同 | **重复** |
| 11 | ToolCtx | `with_work_priority` + `with_identity` L435 | 同(R-141 两键) | **重复** |
| 12 | RunnerConfig | 手写 L449(recall/execution_policy/ask_policy/halt) | `build_runner_config`(同字段+reasoning_override) | **漂移**(两个构造函数,字段重复) |
| 13 | session/输入准入 | `create_session`/`admit_input`/`promote_next_queue` L474 | `create_session`/`admit_input`/`promote_next_input` | **漂移**(核心重复;两个 promote 名待核是否同义) |
| 14 | typed writer | `TypedSessionWriter` + 250ms flush task L516 | 同(R-241 共用契约) | **重复**(契约已共用) |
| 15 | prior 恢复 | `latest_event` + `filter_message_history` L538 | `conversation::recover_messages` | **漂移**(两套恢复逻辑) |
| 16 | subagent runtime | 内联 L743 | `build_subagent_runtime`(execution.rs) | **重复**(几乎逐字节:fast/compact/primary/tier/max_tokens) |
| 17 | 事件汇 | 终端 `on_event` L557(逐行转印) | `build_event_handler`(UiEventSink/TypedEventSink/TraceSink/MetricsSink) | **有意差异=EventSink 注入点** |
| 18 | 询问路由 | 终端 `ask` L676(交互读 stdin + 非交互策略 R-183) | `build_ask_handler`(UI + pending asks 表) | **有意差异=AskRouter 注入点** |
| 19 | 记忆预检索 | `prompt_hints` L815 | run_execution_loop 内同函数 | **重复**(共用) |
| 20 | run_once | 直调 `run_once` L841 | `run_execution_loop`(含 scout/复核流水线) | **漂移**(CLI 无流水线;桌面有) |
| 21 | Ctrl-C/停止 | `tokio::select! ctrl_c` → `finalize_interrupt` L855 | halt_token(协作式停止) | **有意**(CLI 信号 vs 桌面 token=RuntimePolicy 注入点) |
| 22 | 轮末落库 | 内联 set_status/append_event/conversation.updated/shadow L874 | `persist_round_outcome` + `finalize_round` | **漂移**(核心重复;episode/harvest 共用但状态/事件写两套) |
| 23 | 轮末采集 | `harvest_end_of_run` L947 | persist 内同函数 | **重复**(共用) |
| 24 | episode 落库 | `append_episode` L971 | persist 内同 | **重复**(共用) |
| 25 | inbox 整理/candidate | `consolidate_memory_inbox`/`reconcile_candidates` L1002 | persist 内同 | **重复**(共用) |
| 26 | backlog 提示 | `backlog_status` → stderr 提示 L1029 | finalize 后 → auto_action payload | **有意**(CLI 提示 vs 桌面 payload) |

### 判定汇总

- **重复(可直接共用,合并零风险)**:4 ResolveCtx、10 route/client、11 ToolCtx、14 typed writer、16 subagent runtime、19 记忆预检索、23-25 轮末采集/episode/candidate 处置。
- **漂移(需先对齐再合并)**:2 配置告警、5 harness 装配、7 dev 提示、8 模型解析、9 proxy、12 RunnerConfig、13 session 准入、15 prior 恢复、20 run_once、22 轮末落库——其中 5/7/12/22 是两端各写一套的完整重复,合并时逐项对齐后再收进 RunService。
- **有意差异(保留,收敛为三个注入点)**:17 事件汇→EventSink(UI vs 终端)、18 询问路由→AskRouter(UI 表 vs stdin+非交互策略)、21 停止→RuntimePolicy(桌面 halt_token vs CLI ctrl_c),外加 1/3/6/26 的输入源差异(env/IPC 参数)保持调用方传参。

### 下一步(批次)

- 批2:漂移项对齐(5/7/12/22 先逐字对齐再抽),抽 RunService 单一编排入口;两端差异收敛为 EventSink/AskRouter/RuntimePolicy 三注入点。
- 批3:CLI main.rs 其余命令(replay_eval/tracker/work/config/lock/worktree)各自成模块,main.rs 收敛为命令分发+装配(验收③ ≤500)。
- 批4:双端真实闭环(验收②)+ 全量验证(验收⑤)+ 机械核验(验收①)+ close。
## 变更记录

- 2026-08-09 初版:三份结构探查(逐文件通读+外部引用 Grep)汇总成批次计划,交自举执行。
- 2026-08-09 节奏修订(用户定调效率优先):批内验证改定向(cargo test -p + 下游 cargo check),全量降频到条目关闭前与发版前,批提交后 push 由 CI 异步全量兜底;执行纪律 1/2 与危险点 A#6 同步,详见 conventions §1.4 与 A-010。

## 验证证据

TODO(各条目交付时回填):每批的 `cargo test --workspace` / 四冒烟全绿记录;外部 API 面零变更断言;拆解前后 `wc -l` 对照表。

## TODO 与后续风险

- run_task 内部拆分、`run_once_with_parts` 内部拆分:另立条目。
- 拆完后的清理条目:零调用 pub 方法处置、invoke_handler 分组注释、`update_tests` 命名(批0 已按域拆)。
- fmt 收敛(R-156)与 clippy 清零(R-146)排在三条拆解**之后**:两者的全仓 diff 会使本文行号地图漂移。
