# kanzei 当前代码与产品体验全面审计

> 日期：2026-10-16  
> 目的：围绕“好用、现代、精简高效”审视当前产品、代码设计、运行可靠性和维护成本。  
> 范围：当前工作树中的桌面 UI、Rust 后端、记忆/工具/执行链、IPC、构建与验证脚本；结合现有审计记录、修复记录、测试索引及实际窗口截图。  
> 结论性质：这是审计和改进建议，不是产品方向的替代决策，也不代表下述待验证项已被复现。

## 1. 结论摘要

**kanzei 的架构底子比界面表面复杂度看起来更扎实，但产品正在承受“能力增长快于收敛”的代价。** 当前已有清楚的 crate 分层、结构化 IPC 契约、记忆真源/派生索引分离、任务级执行事实与取消机制，也有数量可观的定向回归测试。主要风险不是“完全没有架构”，而是：复杂状态跨层传播、若干失败路径会被呈现为成功或正常无结果、重要控制的发现性不足，以及旧能力/注释承诺/真实统一入口之间仍有不一致。

首轮代码审计发现 **43 项**：P1 7、P2 32、P3 4；分类为确认缺陷 19、设计债 18、待验证风险 5、可靠性风险 1。首轮 38 项之后补入 5 项。现有 round 1/round 2 修复账本对 43 项均记录为 `fixed`；验证索引记录 **72/72 检查通过**，含工作区测试、fmt、clippy 与 UI/IPC 定向检查。**这是脏工作树上的验证，不是干净 HEAD 的发布证据；发布证据标志为 false。** 第 5 节列出随后一次静态后端审查确认的 8 项残余问题；第 11 节是本轮补充的基础 Harness 静态审查，另列 H-1～H-8 共 8 项风险与设计债。它们尚未被实现修复或运行时验证。

从用户目标看，最值得优先投入的不是继续增加页面或开关，而是：

1. **先消除会丢数据、假报成功、误判为正常的路径。** 可靠性直接决定“好用”是否可信。
2. **让当前运行状态和安全控制可见且可快速制动。** 低频细项可折叠，但自动运行、暂停、权限模式不能让用户猜。
3. **把架构承诺收敛到真正统一的实现边界。** 有单一真源是优势；出现“文档称统一、调用仍分散”时，新增功能容易漏掉权限、观测或错误处理。
4. **减少同时呈现的状态与入口，分层而不是删能力。** 对话、项目、活动、研究、文件和调试能力应按当前任务逐步展开。
5. **用真实场景验证“省心”，而不只以源码和 smoke test 判定。** 当前回归覆盖丰富，但未替代首次使用、长时间运行、读屏器、真实 LAN 安全和原生窗口交互验收。

## 2. 审计基线与证据边界

- 首轮审计快照锚定 HEAD `e5a0d02c86f04462b6aa31495f11b05c16b153b5`。该仓库当时工作树已经很脏；审计结论描述的是该时点工作树，不把问题归因到某个提交或某个开发者。
- 38 项首轮 findings 有 115 个源码证据锚点；快照校验记录固定窗口内 tracked source drift 为空。后续补充 5 项后形成 43 项总清单。
- `output/design-audit-20261016/` 中保存了审计机器材料、逐项 repair 记录与验证日志。这些是本机审计材料，不应被误认为都已纳入 Git。本文保留主要结论与来源路径，避免只依赖临时 JSON。
- 72/72 验证记录来自当前脏工作树：包括 `cargo test --workspace`、`cargo fmt --all -- --check`、`cargo clippy --workspace -- -D warnings`，以及 UI、IPC、构建策略等检查。本文没有重跑这些命令；不能据此宣称发布、安装或干净提交验收完成。
- 后端子任务的 8 项新增发现为**只读源码核查**，没有运行测试/构建。本文回读了相关实现并收窄了适用范围；以下仍须测试确认影响。
- 侧栏“375px 时抽屉只有 15px”的初步报告没有列为缺陷：桌面窗口 `tauri.conf.json` 最小宽度为 800px，该窄视口并非当前桌面应用可达状态。若未来要支持独立窄屏/PWA，应另测那条产品路径。
- 截图只代表一次当前任务窗口状态，不代表默认用户配置或典型用户体验。

## 3. 产品体验判断

### 3.1 做得好的地方

- **能力有实际工作流，不止聊天外壳。** 项目与对话、文件编辑、需求/缺陷、记忆、Skills、并行线路、运行画像、研究空间、预览与语音都有可达实现。
- **当前视觉方向已有统一感。** 暖中性表面、克制提示色、项目/会话上下文、活动侧栏与主工作区大体使用同一套交互语言。当前窗口截图显示多列工作台有明确的区域分工。
- **危险操作有不少真实保护。** 文件编辑包含只读识别、内容指纹比较交换、冲突处理与草稿恢复；工具有权限、围栏、取消和终态；记忆以 Markdown 为真源，SQLite/FTS 是可重建派生物。
- **桌面与模型执行不是简单堆在一个文件里。** `kanzei-base`、`harness`、`core`、`llm`、`memory`、`tools`、`app` 的职责边界可辨识；IPC registry 与生成的参数结构也有签名检查。
- **维护者已建立较强的回归习惯。** 72 项验证通过、UI 多项浏览器/契约冒烟、IPC 检查和全工作区测试，为快速演进提供了较好的安全网。

### 3.2 离“好用、现代、精简高效”最明显的距离

#### A. 信息密度与主任务聚焦仍需收敛

当前工作台能同时展示项目/对话导航、主聊天、工具轨迹、活动/子代理等多层信息。对重度用户有价值，但当用户只是“描述目标并确认结果”时，多个长期面板和大量过程细节可能争夺注意力。建议默认把**当前任务、下一步、阻塞/风险**设为视觉主层，工具调用、子代理细节、事件轨迹作为可展开的次层；已发生的失败保持常驻可见，不能为了简洁隐藏失败。

这不是单纯减按钮：应优先统一“任务状态 → 当前阻塞 → 一步可执行操作”的层级，减少相同信息在聊天、活动面板、运行状态与通知中的重复表达。

#### B. 重要状态比低频设置更应常驻

当前 `index.html` 将自动放行、自动推进、暂停/本轮后停、目标条件、模式和子代理控制放进 `composer-more-menu` / 自动推进弹出面板。自动推进当前状态与“停止/暂停”要先发现“更多”并展开后才完整呈现。低频细项放菜单是合理的，但运行是否会自动继续、当前权限档位和一键暂停/停止属于安全与预期管理，不应依赖用户记住菜单位置。

建议常驻窄状态芯片：`自动推进：开/暂停/本轮后停`、权限模式、当前轮次；点击状态进入细项。必须保留输入区现有清爽度，但在自动运行期间提供直接制动入口。

#### C. 页面可达不等于可发现

`00-views.js` 注册的需求、并行线路、文件、架构与运行画像等页面，并非都在常驻 rail 上；命令面板可以到达它们。对老用户，命令面板高效；对新用户，若不了解快捷键或页面命名，可能不知道能力存在。建议提供一个轻量、稳定的“项目工具”导航组或项目内二级入口，按频率展开；不建议把所有页面都塞回 rail。

#### D. 可访问性应成为“现代”基线

- 主聊天消息区当前 `#messages` 没有 `role="log"` / `aria-live`，消息追加路径也没有可供读屏器理解的实时播报语义。不要将每个流式 token 逐字播报；可增加只播报“助手开始回复 / 回复结束 / 发生错误”的短状态区。
- 命令面板输入框是 `role="combobox"`，候选项是 `role="option"` 并有 `aria-selected`，但当前候选关联没有 `aria-activedescendant` 等明确的活动项语义。键盘能操作不代表读屏用户知道当前选中了哪一项。
- 以上是代码语义审查，未用 NVDA/JAWS/Windows Narrator 等真实组合验证；应加入读屏器手工验收，而不把静态属性检查当完整通过。

#### E. 延迟应有状态，而不是空白

首次打开文本文件时，`17-files-editor.js` 先隐藏 placeholder、显示编辑容器，再等待 `loadMonaco()`。若编辑器首次加载偏慢，用户会看到空区域，缺少“正在加载编辑器”的反馈。建议加明确的轻量加载态、取消/失败说明和可重试入口；对缓存命中保持即时，不增加每次打开的视觉噪音。代码路径明确，实际空白持续时间尚未在慢机器实测。

## 4. 43 项首轮发现与修复记录

下表是审计清单的压缩索引。`fixed` 是现有 round 1/round 2 repair 账本中的最新状态，不等于本文重新逐项重跑所有测试；汇总验证边界见 §2 与 §9。

| ID | 优先级 | 发现摘要 | 主要证据位置 | 最新 repair 记录 |
|---|---:|---|---|---|
| UI-01 | P1 | 设置保存完成后会覆盖保存期间的新输入 | `crates/kanzei-app/ui/16-settings.js:1085` | fixed |
| UI-02 | P1 | 设置未成功读取时仍允许保存未知基线 | `crates/kanzei-app/ui/16-settings.js:800` | fixed |
| RUNTIME-01 | P1 | 模型截断、拒绝和步数耗尽被记为“任务完成” | `crates/kanzei-core/src/runner/drive/history.rs:78` | fixed |
| RUNTIME-02 | P1 | 后台退出以固定一秒等待代替运行静止确认 | `crates/kanzei-app/src/runtime_service.rs:449` | fixed |
| ENG-02 | P1 | 发布树干净检查遗漏根目录构建输入 | `scripts/package.ps1:130` | fixed |
| UI-12 | P1 | 发送未被接收时留下外观正常的用户消息 | `crates/kanzei-app/ui/08-compose-runtime.js:527` | fixed |
| UI-13 | P1 | 事件连接与启动意图缺少可见收敛路径 | `crates/kanzei-app/ui/01-core.js:286` | fixed |
| UI-04 | P2 | 管理页异步回执缺少页签与挂载版本校验 | `crates/kanzei-app/ui/30-management.js:78` | fixed |
| UI-05 | P2 | 需求快照读取失败被转换成永久加载或无提示旧数据 | `crates/kanzei-app/ui/25-softwire.js:413` | fixed |
| UI-06 | P2 | 切换语言后管理页保留旧语言内容 | `crates/kanzei-app/ui/30-management.js:32` | fixed |
| UI-07 | P2 | 子代理输入框绕过统一输入法组合态判断 | `crates/kanzei-app/ui/27-agent-team.js:79` | fixed |
| TOOLS-01 | P2 | Shell 门禁把命令参数里的普通文字当成实际写操作 | `crates/kanzei-tools/src/bash.rs:29` | fixed |
| TOOLS-03 | P2 | 流程阻塞虽有独立终态，旧指标仍统计成工具故障 | `crates/kanzei-harness/src/tool.rs:10` | fixed |
| TOOLS-04 | P2 | Tracker 关闭门禁按自然语言出现次数累加验收义务 | `crates/kanzei-tools/src/tracker/actions/action_helpers.rs:102` | fixed |
| TOOLS-05 | P2 | 记忆索引以 ID 代替内容版本，修改正文后可能按旧内容检索 | `crates/kanzei-memory/src/memory/store.rs:979` | fixed |
| TOOLS-07 | P2 | 记忆检索故障被吞成正常无结果，还提示再记一条 | `crates/kanzei-memory/src/memory/index.rs:226` | fixed |
| ENG-03 | P2 | 前端依赖变更没有触发对应验证集合 | `scripts/verify-policy.mjs:66` | fixed |
| UI-03 | P2 | 主对话缺少统一的草稿恢复能力 | `crates/kanzei-app/ui/03-workspaces.js:21` | fixed |
| UI-08 | P2 | 基础前端层与具体页面形成大规模循环依赖 | `crates/kanzei-app/ui/01-core.js:1` | fixed |
| UI-09 | P2 | 工作区改版仍依赖隐藏/搬移旧界面，缺少迁移边界 | `crates/kanzei-app/ui/30-workspace-ui.js:10` | fixed |
| UI-10 | P2 | 页面刷新与后台轮询分散，缺少统一请求/订阅生命周期 | `crates/kanzei-app/ui/12-docs-pages.js:166` | fixed |
| RUNTIME-04 | P2 | 流式事实写入反复开库并扫描历史，运行越久越重 | `crates/kanzei-core/src/store/typed.rs:616` | fixed |
| RUNTIME-05 | P2 | 读取最新对话片段仍全量加载该身份全部事件 | `crates/kanzei-core/src/store/typed.rs:668` | fixed |
| RUNTIME-06 | P2 | 独立执行后台仍依赖隐藏 WebView，服务边界未拆开 | `crates/kanzei-app/src/main.rs:178` | fixed |
| RUNTIME-08 | P2 | 通用执行循环硬编码代码交付话术，任务预算改变输出契约 | `crates/kanzei-core/src/runner/drive.rs:582` | fixed |
| TOOLS-02 | P2 | 知识库增长到固定阈值会让该项目 bash 工具停用 | `crates/kanzei-tools/src/managed.rs:14` | fixed |
| TOOLS-08 | P2 | 按模块读取知识仍先加载全部历史并重复扫描代码树 | `crates/kanzei-tools/src/project_knowledge.rs:163` | fixed |
| ENG-04 | P2 | 运行时冒烟人为扩大全局作用域，需正则守卫补洞 | `scripts/ui-runtime-smoke.mjs:2753` | fixed |
| ENG-05 | P2 | 检查集合隐藏在其他检查内部，执行/覆盖口径难统一 | `scripts/ui-lint-smoke.mjs:1` | fixed |
| OPS-01 | P2 | 应用更新缺少可回到界面核对的安装结果 | `crates/kanzei-app/src/update.rs:84` | fixed |
| OPS-03 | P2 | 设备凭据仍使用自制时间混合算法 | `crates/kanzei-app/src/mobile.rs:32` | fixed |
| RUNTIME-03 | P2 | 后台 RPC 建连后没有响应期限，可能冻结事件恢复 | `crates/kanzei-app/src/runtime_service.rs:157` | fixed |
| RUNTIME-09 | P2 | 模型已发完成事件，执行循环仍等 HTTP 连接关闭 | `crates/kanzei-llm/src/client.rs:310` | fixed |
| TOOLS-06 | P2 | 记忆全文索引重建没有数据库事务，读者可能看到半成品 | `crates/kanzei-memory/src/memory/store.rs:923` | fixed |
| TOOLS-09 | P2 | 读取 Skills 清单隐含写入安装，失败可能阻止聊天装配 | `crates/kanzei-harness/src/skills.rs:95` | fixed |
| OPS-02 | P2 | 手机桥接用手写 HTTP 与无限制连接线程承担请求生命周期 | `crates/kanzei-app/src/mobile.rs:151` | fixed |
| UI-11 | P2 | 回退/分叉按钮误触发复制 | `crates/kanzei-app/ui/05-conversation-actions.js:8` | fixed |
| UI-14 | P2 | 会话转换允许覆盖派生状态不变量 | `crates/kanzei-app/ui/03-shell.js:50` | fixed |
| ENG-06 | P2 | IPC 注册、参数和跨宿主分发依靠多处手写契约 | 补充审计；IPC 构建/同步脚本 | fixed |
| TOOLS-11 | P3 | `memory_search status=any` 在下层被重新解释成仅 active | `crates/kanzei-memory/src/memory/tools.rs:48` | fixed |
| RUNTIME-07 | P3 | 未使用的 LineRuntime 骨架保留占位回收契约 | `crates/kanzei-app/src/run/execution.rs:90` | fixed |
| TOOLS-10 | P3 | 多来源同名 Skill 被静默覆盖，用户无法选择被遮住项 | `crates/kanzei-harness/src/skills.rs:107` | fixed |
| ENG-01 | P3 | 发布证据缺少逐项结果自洽校验 | `scripts/verify-policy.mjs:97` | fixed |

## 5. 当前残余问题：静态后端复核新增 8 项

这些发现不是上表 43 项的重复，也没有被上述 repair 状态覆盖。下面按用户影响排序；除“数据迁移丢失”外，严重度基于合理影响，不代表已发生事故。所有结论来自静态源码核查，需补定向测试。

### P1：legacy 记忆迁移失败时可能不可逆丢失源数据

**证据：** `crates/kanzei-memory/src/memory/migration.rs:18-68`；`MemoryStore::project` 调用路径在 `crates/kanzei-memory/src/memory/store.rs:156-160`。迁移逐条调用 `write_entry`，忽略错误；刷新派生索引错误也忽略；最后仍把 `.kanzei/project/memory.md` 原件替换成迁移说明。若单条落盘/锁/磁盘写入失败，未迁移条目的唯一源文件可能被覆盖。当前迁移模块未覆盖失败路径。

**建议：** 迁移需有显式 `Result`；先写入临时/目标文件并回读验证全部条目，索引刷新结果按明确契约处理；全部成功后原子归档/替换旧文件，同时保留可恢复备份。对旧 ID 冲突、坏条目和空 ID 也应明确报告，不可静默跳过后宣布完成。

### P2：无 `project_dir` 的 `stop_run` 会报告成功却不停止

**证据：** `crates/kanzei-app/src/commands/run.rs:190-269`。缺少项目路径时，函数会收集所有 runtime；但停止与持久化逻辑被包在 `target_project.clone().map(...)` 中，`None` 时不执行 `stop_runtime_and_finalize`，之后仍发出 `kz:stopped` 并返回 `Ok(())`。当前前端已检索到的 5 个 `stop_run` 调用均传 `projectDir`，所以**不是已证实的常用 UI 路径故障**；风险集中于兼容调用、外部 IPC 或遗漏参数的后端调用者。

**建议：** 若只允许单会话停止，就将 `project_dir` 定为必填并拒绝缺参；若支持全局停止，则逐个 runtime 执行收口，并报告成功/失败数，不应“假成功”。补缺参、无运行、多个运行与部分失败测试。

### P2：LAN 桥接对配对码和设备 token 使用明文 HTTP

**证据：** `crates/kanzei-app/src/mobile.rs:647-710,111-129`。LAN 模式绑定 `0.0.0.0`，返回 URL 仍是 `http://`；配对认证与后续设备 bearer token 经普通 HTTP 请求传输。LAN 模式为显式选项且默认回环，这降低了默认暴露面，但在共享/不可信网络中，同网段监听者可窃听凭据或篡改请求。

**建议：** 为 LAN 提供 TLS/安全隧道，或者默认不开放 LAN；若暂时保留明文模式，在 UI 启用时给出不可忽略的风险说明，缩小可执行操作并支持会话级凭据轮换。应有抓包/威胁模型验收，不能只用本机 HTTP 冒烟证明安全。

### P2：失败记忆召回的 30ms 检查没有给实际检索设截止时间

**证据：** `crates/kanzei-memory/src/memory/mod.rs:718-742` 仅在建立索引前检查一次计时；随后同步构造 `SqliteMemoryIndex` 并执行搜索。构造路径 `memory/index.rs:130-147` 会建立指纹索引、加载条目；搜索路径还有 SQLite/一致性检查。慢盘或增长中的记忆库可能仍阻塞主执行循环。已有“超时”测试只验证空库 miss，不能证明真实耗时路径受限。

**建议：** 尽量复用本轮记忆快照/索引；若需要 30ms 保证，给实际检索一个可取消的截止时间或隔离执行，并以较大数据集、慢 I/O/锁竞争测试。否则去掉“超时降级、不阻塞主循环”的承诺，避免把预算检查误称为硬上限。

### P2：novelty 检索错误被伪装成“新知识”

**证据：** `crates/kanzei-memory/src/memory/retrieval/search.rs:147-170` 仅在搜索返回 `Ok` 时分析候选，错误被跳过后会走 `Novelty::New`；`crates/kanzei-memory/src/memory/tools.rs:297-323` 对 Duplicate 以外结果继续写 inbox。因此 SQLite/索引一致性故障会被用户感知为正常“noted”，同时可能造成重复候选与后续人工清理。

**建议：** 分类结果应区分 `New`、`Duplicate/Uncertain` 和 `Unavailable/Error`；检索故障默认明确失败或写入“未判定、待重试”独立状态，不得转译为“新”。给索引故障注入测试，并断言用户收到的消息与持久化状态一致。

### P2：并行工具成功权限决策缺少审计事件

**证据：** `crates/kanzei-core/src/runner/drive/parallel_tools.rs:134-154` 并行预检只在 `Deny` 时发送 `PermissionResolved`；串行路径 `drive/permissions.rs:47-88` 对 allow、session approval/rule、auto allow 等也写入决策事件。由此成功并行工具的授权来源轨迹可能缺失，审计数据依赖执行路径。

**建议：** 串/并行共用权限评估结果及事件记录函数；每个实际执行的敏感工具都能回溯 decision、source 和 rule。用串行与并行同一组规则/会话批准输入做事件等价测试。

### P2：Tool Pipeline 是部分共用骨架，不是完整权威执行入口

**证据：** `crates/kanzei-harness/src/tool_pipeline.rs:17-34,67-145` 同时定义阶段、`wrap_execute` 与 `run_tool_pipeline`；实际调用者目前只有 `bash`、`git`、`glob`、`grep`、`read` 等少数工具（`crates/kanzei-tools/src/`）。权限裁决仍在 core 的 `drive/permissions.rs`，输入解析仍由各工具处理，托管围栏与部分结果策略在其他路径。它不只是 `wrap_execute`，但也尚非覆盖全工具的统一 pipeline。

**建议：** 二选一并兑现：逐阶段迁移并形成可验证的公共执行入口，或者把模块命名/注释明确收窄为“工具包装和可选后处理骨架”。阶段列表应标清哪些是规范要求、哪些只是预留；否则新工具难以判断必须接哪些安全/观测能力。

### P3：移动服务启动响应返回伪设备元数据

**证据：** `crates/kanzei-app/src/mobile.rs:719-728` 从 SQLite 读取 name 与 paired time 后以 `_name`/`_paired_at` 丢弃；`778-792` 启动结果为每台设备固定名称“已配对设备”和 `paired_at_ms: 0`。设置页启动后会再调用设备列表刷新（`ui/16-mobile.js:63-70`），所以通常很快被修正；直接消费 `mobile_service_start` 返回值的其他调用者仍会得到错误契约。

**建议：** 在启动结果中返回刚读到的真实记录；或移除重复的 devices 字段，统一要求调用 canonical `mobile_device_list`。

## 6. 其余可用性与质量风险（未等同确认缺陷）

1. **命令面板可操作但读屏状态不完整。** `ui/index.html` 的 combobox/listbox 和 `ui/21-palette.js:220-263` 当前有 `aria-selected`，但没有明确的活动候选关联；应补语义并以键盘/屏幕阅读器验证。
2. **首次打开文件的加载等待缺少明示状态。** `ui/17-files-editor.js:448-460` 在 Monaco 完成加载前已显示编辑容器；慢机器上可能像空白。补浏览器节流与原生冷启动观察。
3. **项目工具依赖命令面板提高发现率。** 这是精简导航的可接受取舍，但应在新用户路径里试验是否能自然发现“文件/并行线路/需求/架构”。
4. **自动推进/权限等安全状态需要更显眼。** 控件藏于低频菜单是当前结构事实，发现性影响尚无用户测试；不应直接推导为所有用户都会误操作。
5. **高密度三列工作台需按任务分层。** 当前截图能看到明确的区块分工，也同时存在导航、主内容、运行活动等并行信息。截图不是默认态/用户研究，建议做任务场景测量后再决定收敛方式。
6. **权限保护与便利性并非二选一。** auto-allow/自动推进可提高效率，但应把“当前模式、下次动作、停止入口”视为同一组安全 UI，而不是只呈现一个开关。

## 7. 架构与设计的总体判断

### 值得保留并继续强化

- **依赖方向清楚：** Harness 定义工具/权限/上下文契约，Core 执行循环与 session facts，Tools 组合具体能力，App 负责桌面及 IPC。不要因代码审计发现局部复杂而把所有执行逻辑重新塞回 App。
- **源数据与投影区分明确：** Markdown 是记忆真源，索引与图是可重建派生物；会话 typed facts 与 SQLite 投影也有事实边界。应把“派生失败不污染真源”和“迁移前源数据仍可恢复”进一步制度化。
- **IPC 正在走向单一注册表：** `command_registry.json`、`build/ipc.rs`、`scripts/sync-ipc-contract.mjs` 与签名检查值得保留；剩余跨宿主/参数定义重复是边界收口问题，而不是需要推翻生成方式。
- **安全原则进入代码，而非只靠提示：** permission、managed fence、取消、内容指纹/冲突处理的组合是自举工具产品必须拥有的防线。后续应优先补齐并行权限事件与 LAN 明文风险，而非降低门槛换取表面简洁。
- **回归用例覆盖有基础：** 多数问题可以用边界测试直接表达；通过统一检查集合与结构化证据避免“测试脚本有很多，但不知道提交实际跑了什么”。

### 对“补丁式代码”的专项判断

不能把所有渐进修改或兼容代码都叫作补丁债。必要的迁移、适配器和渐进替换是正常工程；判别点是：**是否有单一状态所有者、失败是否显式、兼容路径是否有删除条件、调用者是否能由测试证明覆盖。** 按此标准，当前有几类值得盯紧的具体信号：

- **静默吞错后继续写/覆盖。** legacy 记忆迁移对条目写入和索引刷新错误忽略，仍替换源文件；novelty 检索错误被解释成 New。它们不是单纯的“代码风格问题”，而是把失败伪装成正常业务结果。
- **可选参数与业务语义不匹配。** `stop_run` 接受可空项目路径，但缺参分支收集了运行态后不执行停止逻辑，最后仍发成功事件。接口表面上支持的状态比实现真正支持的多。
- **只完成局部统一，却使用全局化命名/承诺。** Tool Pipeline 有共享骨架，但权限、解析与部分结果策略仍在其他路径；容易在增加新工具时靠“再补一处”维持一致。
- **兼容层或旧骨架需要明确退出条件。** 首轮审计中曾有旧工作区 UI 隐藏/搬移、未使用 LineRuntime 骨架等问题，当前 repair 记录显示已处理。此类迁移方向是对的，但应持续验证旧入口是否真的删除，而非继续双轨维护。
- **复杂度集中在大文件，不等于必须机械拆分。** 文件图显示 `kanzei-memory/src/memory/mod.rs`、`kanzei-app/src/mobile.rs`、`ui/index.html` 都是较大的协调点（约 3050、2323、1710 行）。行数包含测试/HTML/不同责任，不能单凭总行数判定巨石；应结合 `kz metrics` 的生产行、最大函数与实际变化耦合，优先抽离有独立状态/错误边界的职责。

改进原则：每个临时兼容路径都要有负责人、迁移条件和删除测试；关键状态转换返回可区分的 `Result`/终态；“共享执行管线”需要可查询的接入覆盖；不要用注释描述尚未强制的保证。这样既避免一次性大重构，也避免小补丁不断堆出第二套真相。

### 需要避免的设计模式

- **伪成功：** 后端未执行却发成功事件、检索失败被当作无结果/新条目、迁移部分失败仍覆盖源文件。
- **弱约束伪预算：** 开始前看一次计时，之后同步无界执行，却声称整个动作有固定耗时上限。
- **名义统一但实际可选：** 共享 API 只被部分工具调用，文档/注释列出完整阶段但没有明确哪些路径强制执行。
- **多处局部状态而缺少唯一所有者：** UI 页面异步加载、运行/会话身份、后台服务退出都要明确唯一的状态 owner、取消与 stale response 处理。
- **把“能通过测试”视为“好用”：** smoke tests 证明具体契约，不会自动证明入口易找、状态易懂、长任务安心或读屏器体验完整。

## 8. 推荐改进顺序

### 第一批：安全与可信度（最高优先）

1. 修复 legacy 迁移事务/失败回滚，保证不覆盖唯一源数据。
2. 明确 `stop_run` 缺参合同并新增对应用例；禁止无动作却回报成功。
3. 修复 novelty 错误分类与失败召回截止/快照策略。
4. 为 LAN 桥接做安全决策：TLS/隧道、默认关闭，或明确且可执行的风险边界。
5. 统一并行/串行权限决策审计事件。

**完成判据：** 故障注入与契约测试证明“不丢数据、不假成功、错误可见”，而不是仅用正常路径冒烟。

### 第二批：日常操作更快且更可预期

1. 自动推进期间将状态、权限档位和暂停/停止入口常驻。
2. 首次打开文件有加载/失败/重试状态。
3. 项目工具按使用频率分层，命令面板仍保留全局快速入口。
4. 以 5 个常见任务观察首次到达时间、完成步数和误触：创建项目并开始任务、继续旧对话、定位需求、查看文件/架构、暂停自动推进。

### 第三批：现代可访问性与信息层次

1. 主对话用简短阶段消息播报，不逐 token 播报。
2. 命令面板补 active descendant / listbox 焦点契约，验证键盘与读屏器。
3. 处理高密度场景的默认层级：主任务优先，轨迹/代理可展开，失败与阻塞仍显著。
4. 收集真实屏幕/窗口尺寸、读屏器和语言切换下的 UX 反馈；桌面最小宽度 800px，不以不可达的 375px 桌面场景作为验收。

### 第四批：架构契约和维护成本

1. 决定 Tool Pipeline 是强制执行器还是局部 wrapper，并让文档、API、调用覆盖一致。
2. 继续降低跨模块循环与状态复制，但以依赖/调用证据为先，不为“文件看起来多”做机械拆分。
3. 对记忆、SessionStore、移动桥接做规模/慢 I/O 基准，优先修有可测退化的路径。
4. 保持生成式 IPC、单一源数据及验证集合，避免手写兼容路径重新扩散。

## 9. 验证状态与未完成验收

### 已有证据

- 首轮 43 个问题的 repair 记录中，最新条目均标记 `fixed`。
- `round2/verification-index.json`：72 项检查 registered=72、passed=72、failed=0；包括工作区测试、fmt、clippy、verify-policy、UI browser/smoke、IPC 与设计检查。
- IPC 源码当前仍有可识别的生成/同步边界；记忆 Markdown 真源及 FTS 派生路径可从当前源码核实。
- 主窗口当前 `minWidth=800`，故 375px 桌面抽屉计算不构成当前窗口缺陷。

### 尚未通过或未执行的验收

- 该验证索引标记 `releaseEvidence=false`；不能据此宣称发布完成、安装包正确或干净 HEAD 通过发布门禁。
- 本文未重跑全量测试；报告生成本身只新增 Markdown，不改 Rust/前端实现。
- 后端新增的 8 项只读发现未修复、未运行针对性测试；LAN 风险更未进行网络抓包或真实威胁模型验收。
- 未对 Windows Narrator/NVDA 等真实读屏组合做主对话、命令面板和自动运行控制验收。
- 未以慢盘、大规模记忆库、SQLite 锁竞争验证 30ms 召回预算，也未在迁移失败条件下证明源文件可恢复。
- 未对 Monaco 冷启动/慢机器做加载体验观察；首屏到达时间、功能发现率、用户任务耗时未做量化研究。
- 当前工作树与审计 HEAD 不等同，且存在其他未提交工作；报告只作当前工作树分析，不将其归因于提交历史。

## 10. 第一轮总体判断

**现在的主要瓶颈不是功能数量不足，而是“能力、状态和执行路径”尚未完全以用户能理解的方式收敛。** 产品已有足以形成现代高效工作台的结构和保护机制；下一阶段应减少状态歧义、提高默认路径发现率、让关键控制可见，并把失败语义、流水线承诺与真实实现对齐。如此才能既保留高级用户需要的能力，又让普通用户无需理解内部架构便能放心使用。

## 11. 补充审计：基础 Harness 与执行边界

### 11.1 总体判断

`kanzei-harness` 的**依赖方向健康，职责面却已明显扩大**。Cargo 直接依赖只有 `kanzei-base` 和通用 async/serde/config 库，不依赖 core、tools、LLM 或桌面端；实际模型循环、provider 协议和权限询问由 core/入口消费 Harness 契约。这是很好的底层边界。与此同时，该 crate 当前 46 个文件、约 13,237 行；`config.rs` 约 2,420 行、`auto_run.rs` 约 1,635 行、`permission.rs` 约 1,381 行、`areas.rs` 约 1,057 行。此处行数含测试及多类责任，不能单凭规模断定质量差，但可以确认它不只是几组工具 trait：还承载配置、权限规则、项目根、区域索引、Skills、编排、后台邮箱、自动推进和托管写围栏等控制平面能力。

公共运行装配以 Base → Dev/Research → middle → Markdown → Config → tail → GeneralChat 为序；桌面与 CLI 在 middle/tail 插入不同能力。`Harness::resolve` 产出每轮不可变快照，按需工具通过 `tool_search` 延迟暴露。相较把运行循环、桌面服务和所有工具塞在一起，这套分层是合理的；当前真正需要收敛的是**契约覆盖与错误语义**，不是推倒重建。

### 11.2 值得保留的基础设计

- **无反向依赖的契约层。** Harness 定义 Tool、ToolCtx、ToolOutput、Ruleset、ContextSource、Component 等，runner 与工具实现依赖它；桌面/CLI 适配器不反向进入 Harness。
- **代码树、项目根和执行身份显式区分。** `cwd`、`project_root`、worktree/project write key、run/process/session identity 不互相代替，减少 worktree 项目状态串线。
- **能力发现可渐进加载。** `materialize_tools` 按整体 deny 摘除工具；deferred loader 可见时才隐藏部分 schema，`tool_search` 自身不延迟；loader 被拒时退回全部允许工具，避免模型看不到工具且无法加载。
- **权限不由模型 schema 代替。** `Ruleset` 有序匹配、默认 Ask、硬 deny 优先；bash 文本与路径资源采用不同匹配语义。Core runner 对调用再次评估权限并处理审批；这道 gate 不依赖 `tool_pipeline` 是否接入。
- **执行并发默认保守。** Tool 默认 `Exclusive`；只有显式声明 `Shared` / `WorktreeWrite` 的工具才加入键控并发。ToolOutput 分开表达 provider 回喂信号 `is_error`、业务 outcome、稳定 code 及给 UI 的结构化 display/artifact。
- **上下文刷新不累积进消息历史。** stable 和每步 refreshable source 分别渲染，报告 source 字符数；`InputInbox` 只在完整工具结果批次后取 steering，异步问题也有单独 durable 路径。App 的 mailbox adapter 会在 lifecycle 锁内复核 generation 并持久化输入。

### 11.3 当前问题与维护风险

#### H-1（P2，边界正确性）：显式选择 agent 时不检查 profile scope 或 Primary/Subagent 身份

`HarnessSnapshot::select_agent(None)` 会筛选当前 profile 可用且 `mode=Primary` 的 agent；但 `select_agent(Some(name))` 只按名称取注册表条目，不核验 `agent.profile.includes(snapshot.profile())`，也不核验是否适合作为顶层 Primary。CLI 直接把 `KANZEI_AGENT` 传给这个显式分支（`crates/kanzei/src/cli/run.rs:225-233`），所以当前 API 可以接受与选定 profile 不匹配的 agent。项目权限仍由 `ResolveCtx.profile` 与 Ruleset 决定，**这不是已证实的权限绕过**；但用户选择的 profile、agent 可用范围及 agent 的角色可能彼此矛盾，配置界面也难以保证所有调用者都预先过滤。

**建议：** 拆分 `select_primary_agent` 与明确的子代理选择入口；显式选择时至少校验 profile scope 和 mode，错误信息列出当前 profile 下可选 agent。若某些内部场景确需跨 scope，使用显式 override 及调用点测试，而非由通用选择函数静默接受。

#### H-2（P2，已确认的错误分类问题）：ToolGuard 拒绝被包装成 Failed

生产 `CommandLengthGuard` 在命令超过 30,000 UTF-16 单元时返回 `Err(String)`（`kanzei-tools/src/bash.rs:26-57`）。`run_tool_pipeline` 对任意 guard 拒绝统一生成 `ToolOutput::error(...)`，不带稳定错误码（`kanzei-harness/src/tool_pipeline.rs:139-149`）；这会得到 `outcome=Failed`，而非 `NeedsCorrection` / 其他预期拒绝。遥测因为拒绝正文以“permission denied”开头，会把这条长度限制误分为 `PermissionDenied`；而普通 metrics 与失败召回因缺少预期拒绝 outcome 机器头，仍会计为失败/发起 Recall。因此明确的“命令太长，请改文件中转”的可纠正输入，同时被错误标成权限拒绝和运行故障。现有 bash 用例检查了拒绝与文案，但没有证明 outcome/code、遥测和 Recall 分类正确。

**建议：** Guard 返回结构化决策（放行或携带 `ToolOutcome`、稳定 code、用户文案的拒绝），或让 Guard 构造 ToolOutput；加端到端测试断言超长拒绝得到预期 `NeedsCorrection`（或等价拒绝类型）、不误归类为 `Failed` / `PermissionDenied`，且不增加失败召回，同时仍将拒绝结果回喂模型。对于真实 body panic/执行错误，仍保留 Failed 分类。

#### H-3（P2，可靠性/可诊断性）：ContextSource 不能区分“本来无内容”和“读取失败”

`ContextSource::baseline` 返回 `Option<String>`，组装报告只有成功注入的 source 名称与字符数。多个开发上下文把读取错误用 `.ok()?` 折叠为 `None`，例如 `dev/ideas`、`dev/decisions` 和 design-index（`kanzei-tools/src/profiles/dev.rs:201-219,313-331,334-357`）。此时本轮仍可能正常发给模型，但 context report 不会显示该 source 加载失败。`dev/conventions` 则专门把非 NotFound 错误写进提示上下文，证明不同源需要不同失败处理；当前抽象无法统一表达“可选为空”“可恢复缺失”“必须可用但加载失败”。

**用户影响：** agent 可能在未加载决定、设计索引或项目背景的情况下继续给出看似正常的方案，用户难以判断它是没找到内容还是源文件读取失败。

**建议：** 不要求任何可选文件失败都停止运行；改为让 source 返回 `Result<Option<ContextChunk>, ContextError>`，元数据注明 optional/required 与降级策略。组装账单记录 omitted/error 与原因；required source 失败时明确提示或阻断，optional source 失败则可继续但向用户/轨迹可见。补断言区分“文件不存在”和“权限/IO 错误”。

#### H-4（P2，架构债）：Tool Pipeline 的宣称阶段与生产覆盖并不一致

`tool_pipeline.rs` 的顶部和 `ToolPhase` 列出 parse/materialize → policy → guards → wrap → body → result policy → observer；但 `run_tool_pipeline` 实际只有 guards → body → result policies → observers。参数解析仍在每个工具 body，permission/Ask 在 core runner，执行取消/进度/panic 的 `wrap_execute` 也是 core 独立调用的另一层。当前 `run_tool_pipeline` 生产调用只核实到 bash、git、glob、grep、read 五类；生产代码没有 `ToolResultPolicy` / `ToolObserver` 实现。R-244 文档将它描述为逐族迁移的骨架，这更准确；但 API/阶段名容易被误读为所有工具已走完整统一入口。

**边界澄清：** 当前这不是权限绕过，runner 的权限 gate 仍单独强制；风险是未来开发者误以为 policy、observer 或 result rules 会自动覆盖所有工具。并且 `ToolResultPolicy::apply(&mut ToolOutput)` 可以改动 `is_error`/`outcome`，而 trait 注释仅称“不改变成功/失败语义”，类型本身没有强制不变量。

**建议：** 明确它的真实定位：要么逐族迁移并把解析、Guard、结果后处理、Observer 接入做成可查询覆盖清单；要么把它命名/文档收窄为“可选 Guard/结果观察 wrapper”。在恢复强契约前，不应把尚未接入的 phase 写成统一保障。对结果策略增加不可变语义或执行后不变量校验；把 pipeline guard 拒绝分类纳入共同测试。

#### H-5（P3，调试性）：Registry 的 last-wins 有意，但不记录覆盖来源

注册表同名后写覆盖、保留原序是明确规则；MarkdownComponent 让项目 agent 覆盖同名全局 agent，也是有用的 overlay。但 `Registry::insert` 只保留 name 与 value，不记录哪个 Component/目录注册了它；意外重名时，最终快照不能解释“谁覆盖了谁”。这不是说应禁止用户覆盖默认 agent，而是显式层叠策略缺少可观察结果。

**建议：** 在 debug/report 路径中记录 name、来源 Component、覆盖链与最终胜者；对核心安全工具名、managed resource 替代工具等高风险覆盖给出严格冲突检查，普通 user override 继续按明确优先级工作。

#### H-6（P3，文档漂移）：现行 M1 文档仍写“没有 skill 工具”和项目 Skill 路径

`docs/design/harness_m1.md` 仍写 Skill 正文无专用工具、通过 `read` 读取，以及 `.kanzei/skills` 项目目录约定（§2、§6）。当前 `MarkdownComponent` 注册 deferred `skill_read`；catalog 实际来自内置、全局 personal 和用户目录 `.codex/.claude/.agents/skills`，项目 Skills 不参与（`kanzei-harness/src/markdown.rs:24-35`、`skills.rs:161-213`）。同一文档对“before_tool_call 权限拦截器链”的旧图景也需与当前 core runner 实际执行位置同步。

**建议：** 只改当前合同，不搬运历史方案：记录 global Skills 来源、按需 `skill_read`、CLI/桌面能力差异及 permission gate 在 core 的真实边界；历史状态/迁移留在文档记录中，避免旧说明继续引导新增代码走错入口。

#### H-7（P3，仅注释问题）：ToolEnd outcome 注释漏列 `blocked_by_workflow`

`RunEvent::ToolEnd` 注释只列 success/noop/needs_correction/needs_confirmation/failed，漏了真实使用的 `blocked_by_workflow`（`kanzei-core/src/runner/event.rs:137-145`）。Child audit 已确认 payload 与 UI 消费均支持该 outcome，故这是注释漂移，不是事件丢失。改注释并在 outcome 枚举扩展时用共享测试检查文档列表即可。

#### H-8（P3，效率扩展风险）：上下文预算能裁剪历史，但固定 system head 无法缩小

`ContextSource` 报告按字符计量；core 的真实预算估算会把 system、工具 schema 与 messages 一起计入（`kanzei-core/src/runner/drive/context_budget.rs:56-64`）。超线时优先 prune/压缩的对象是 messages（同文件 `:67-95`）；system baseline、agent prompt 和当前工具 schema 在这条路径中保持不变。当前延迟加载工具 schema、按 source 拆分 stable/refreshable baseline、memory/context 账单都是有效的成本控制；但 conventions 按项目规则完整注入，design index 与决策上下文也可能随项目资料增长。若固定 head 本身逼近预算，继续压缩对话历史不能解决问题，最终只能进入 futile-compaction / provider overflow 恢复路径。

**判定边界：** 本轮没有复现常规项目因 system head 过大而溢出；这是随上下文目录和工具 schema 增长的扩展风险，不是当前普遍性能缺陷。字符账单也不是 token 账单，token 预算要以 runner estimator/provider usage 为准。

**建议：** 按 source 记录估算 token、是否必需、优先级、加载耗时和降级策略；保留当前必须完整注入的项目约定，不要静默截断；对可选设计索引/历史决策提供精简摘要与按需读取，预算不足时明确指出是哪部分固定 head 占满空间。用大规模 fixture 验证“mandatory system + tool schemas”本身超过阈值时能尽早给出可行动提示，而不是空转压缩旧消息；refreshable file sources 可按 mtime/hash 缓存并测量单步组装耗时。

### 11.4 面向用户目标的 Harness 改进顺序

1. **先修错误语义：** Guard 拒绝要带稳定 outcome/code，并打通 UI、失败指标、Recall；不让可修正的输入增加“工具故障”噪音。
2. **让关键上下文缺失可见：** ContextSource 可选/必需分层、IO 失败有迹可循；上下文完整性比再堆几条 prompt 提示更能提升输出可靠性。
3. **把选择合同做实：** 校验显式 agent 的 profile/mode，agent 目录显示当前 profile 可用状态；CLI 和桌面共用同一断言。
4. **Pipeline 先定范围，再加扩展点：** 目前共享 wrapper 与 runner policy 各司其职，先列出真实执行矩阵，再决定是否值得让每种工具迁移。若扩展点没有生产用例，优先删除/标为预留，而不是新增抽象。
5. **补来源可观察性和设计文档：** Registry override 审计记录；同步 harness_m1 当前合同。
6. **规模治理以实测驱动：** 现 crate 大但依赖干净；只把已形成独立变化原因/故障边界的模块拆开。不要为了“13k 行”马上拆 Cargo crates，先用变更耦合、编译成本和可独立验证性证明收益。

## 12. 网络调研：成熟 Agent Harness 的共同原则

> 调研时间：2026-10-16。官方文档用于核对各家公开的设计/SDK 合同，不是独立安全评测，也不证明其实现一定优于 kanzei。不同框架的 API 与版本会变；下列内容是可借鉴的原则，不应逐字照抄。

### 12.1 调研结论及与 kanzei 的映射

| 公开资料中的原则 | 对 Harness 的启示 | 当前 kanzei 对照 |
|---|---|---|
| Anthropic《Building effective agents》（2024-12-19）建议从最简单可用系统开始；工作流适合预先定义步骤，agent loop 适合需要模型动态决策的任务；增加复杂度应由评测收益证明。工具接口和参数要清楚、可测试。 | 把权限、解析、超时、取消、结果策略的“唯一入口”分层定义；不要因想做统一而预留七个阶段却只有部分实际执行，也不要只为抽象美观拆 crate。用回归/用户任务验证每一层的收益。 | crate 依赖方向清楚；但 tool_pipeline 的阶段名超出真实覆盖。应明确哪些是硬契约、哪些是计划中的扩展。
| Anthropic《Effective harnesses for long-running agents》把跨会话交接视为核心问题，示例使用进度工件与 Git 历史、每次只推进可验证的小步；文章也承认单体 agent 与多 agent 哪个更优尚不确定。 | Durable 状态、交接和源码/任务事实要有真源；不要让 Harness 的内存邮箱被误当持久队列。按任务/事件合同逐步恢复，完成后留可验证工件。 | Work Unit/session facts 与 app 的 durable mailbox 把持久状态放在 core/app，而 Harness 提供低层契约，边界基本正确；后续要继续保持这一分工。
| Anthropic《Trustworthy agents in practice》强调模型可能误读意图并受 prompt injection 影响，单一模型训练不是保证；要多层限制工具与环境，提供计划审阅和执行中干预，并明确安全仍非绝对。 | “模型可见工具 schema”不等于“获准执行”；需要 fail-closed 的权限 gate、作用域限制、执行后证据和用户制动入口。 | Ruleset/hard deny 在 runner 执行前强制，并与 UI permission events 分离，符合分层思路。新增 H-2 表明低层 guard 仍需保持结构化终态；Pipeline 本身不是权限授权层。
| OpenAI Agents SDK 的工具文档把 JSON 参数 schema 与 runtime `is_enabled`、input/output guardrails、`needs_approval` 明确区分；运行异常也区分抛异常导致 run 失败和返回模型可见错误结果。 | schema 负责告诉模型“如何调用”；policy 决定“是否能做”；guard 决定“这个具体输入是否合规”；审批负责暂停等待用户；结果码不能把输入拒绝伪装成运行异常。 | kanzei 有 Tool::input_schema、Ruleset、ToolOutcome 和 runner Ask/approval 分层；但 ToolGuard 只有 `Result<(), String>`，当前造成拒绝被标成 Failed，需要收紧结果类型。
| OpenAI 的 handoff/session 文档指出转交 agent 时默认历史范围、过滤、授权是敏感边界；session id 本身不等于认证授权，应用必须控制谁能访问会话。 | 传递执行身份与历史时要显式定义所有权、可见范围、来源及授权；不要把 `session_id` 当安全凭证。 | ToolCtx 明确区分 session、process、run 与 project roots，且 app mailbox 在 lifecycle 锁内核验 generation；这是良好基础。仍应保持持久数据访问授权在 app/store 层，而非 Harness 类型暗示已自动保证。
| OpenAI tracing 文档指出 trace 默认可能包含 LLM 输入/输出、工具参数与结果；可关闭敏感数据采集，但 ZDR 组织无法使用 tracing，页面未声明一般保留期限。LangGraph persistence 文档区分 thread checkpoint 与跨 thread store，并指出内存 saver 在进程重启后不持久；应用需选择持久化实现。 | 运行日志、tool inputs/outputs、checkpoint 与短期 mailbox 要分开评估保留期、敏感数据和恢复保证；必要时做脱敏和访问控制。 | kanzei 的 session store 与 mailbox 已分层。本文未完成对所有持久 trace/tool payload 的隐私保留审计，也不把 harness 侧 mailbox 单独描述为 durable。

### 12.2 可直接采纳的原则

- **以真实完成路径定义“统一”。** 一项安全/观察能力只有在所有该适用的执行路径都会触发、并有漏接检测时，才是系统合同；当前 pipeline 有完整 runner 级 permission gate，但可选 `ToolObserver` 不是全工具遥测合同。
- **对 agent 和工具权限分层。** 模型选择名称、Schema 合法、agent system prompt 写“只读”，都不能替代运行时 Ruleset/Guard。角色标签应在装配入口校验，不要依赖 prompt 表明身份。
- **将拒绝和故障在第一处区分。** 用户无需理解 tool pipeline，但系统遥测、Recall、UI、模型重试都依赖稳定 outcome。低层遇到“输入不合规/政策拒绝/流程阻塞/运行异常”应有不同类型。
- **长任务交接的最小真源。** Kanzei 已有 Work Unit 和 session 事件，不宜再造 Harness 自有的第二套进度文件；Harness 只提供 callback 与执行环境合同，持久恢复由 core/app 明确负责。
- **安全可见性和数据最小化一起评估。** 记录审批与工具结果有助排障，也可能含命令、路径、文档片段或凭据。应审计数据具体写到哪、访问权限与保留期，而不默认“有 trace 就一定安全”。
- **官方建议是原则，不是对比胜负。** Anthropic 的文章明确指出 agentic 系统会牺牲延迟/成本，且多 agent 优势没有普遍结论；OpenAI/LangGraph 文档描述各自 SDK 的行为边界。本轮没有找到可独立证明这些产品更可靠/更易用的同口径基准，故不以品牌结论代替本地测量。

### 12.3 来源

1. Anthropic, **Building effective agents**, published 2024-12-19：<https://www.anthropic.com/engineering/building-effective-agents>。重点为简单系统优先、workflow/agent 区分、评测后再增加复杂度、工具接口清晰与沙箱测试。
2. Anthropic, **Effective harnesses for long-running agents**：<https://www.anthropic.com/engineering/effective-harnesses-for-long-running-agents>。重点为 session 间交接、增量工作、进度工件与 Git 恢复；文中也说明单 agent 与 multi-agent 的普遍优劣尚未确定。
3. Anthropic, **Trustworthy agents in practice**：<https://www.anthropic.com/research/trustworthy-agents>。重点为 prompt injection、工具权限、人工审阅/介入及多层防护的限制。
4. OpenAI Agents SDK, **Tools**：<https://openai.github.io/openai-agents-python/ref/tool/>；**Handoffs**：<https://openai.github.io/openai-agents-python/handoffs/>；**Sessions**：<https://openai.github.io/openai-agents-python/sessions/>；**Tracing**：<https://openai.github.io/openai-agents-python/tracing/>。重点为 schema 与审批/guardrails 分离、handoff 历史过滤、session 授权责任与 trace 敏感数据边界。
5. LangChain, **LangGraph persistence**：<https://docs.langchain.com/oss/python/langgraph/persistence>。重点为 thread-scoped checkpoints、跨 thread store、内存 saver 的非持久性及应用侧选择。该文未定义 side effects 的幂等保证，本报告不从中推导幂等结论。

## 13. 更新后的结论

基础 Harness **不需要被推翻，但需要从“大而全控制层”继续收敛为可信、可观察、合同与实现一致的基础设施**。最直接的用户价值来自三件事：让低层拒绝拥有正确终态、关键上下文未加载时明确可见、显式 agent 选择遵守 profile/角色边界。之后再决定是否把 tool_pipeline 扩成强制总线；不能先做更大抽象，再假设所有工具已经得到同一保护。


