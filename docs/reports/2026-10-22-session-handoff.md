# 交接报告：模型决定与用户确认的分账（D-764①②③、D-766①②、前端诚实性）

- **日期**：2026-10-22（CST，最后一次验证 15:40–15:43）
- **分支 / 基线**：`dev`，HEAD `ea4f15600aa43424d8db179a1dcf4d9018330548`
- **工作树**：**未提交**，且正被另一个并发会话写入（见 §4）。本报告描述的是 15:55 时点的工作树状态，不是 HEAD 的内容。
- **边界**：本轮只做代码/脚本修复与本机定向验证。**没有** commit、push、删产物、改受管项目文档（`conventions.md`、tracker），也**没有**跑 `verify.ps1 -Full`（原因见 §4）。

> 后续接手已补齐规范、确认凭证与真实 Git 回归，并通过工具关闭 D-764/D-766，见 §8。§0–§7 保留原交接时点的观察；其中“需授权”“未修改受管文档”等描述不再代表后续状态。

## 0. 一句话

把「模型自己说的话」和「用户真的确认过」在引擎层分开：引擎投递的续跑文本带归属前缀、压缩纪要不把它当用户指令、tracker 的语义确认门禁只认引擎签发的作答凭证；同时让设计时效门禁判已提交树，并把 `.kanzei/project` 纳入构建输入。

## 1. 本轮修复

### 1.1 D-764①：引擎续跑的归属写进内容本身

- 新增常量与幂等函数 [`auto_run.rs`](../../crates/kanzei-harness/src/auto_run.rs#L173)：`ENGINE_ROUND_MARKER`、`mark_engine_round()`。已带标记的输入原样返回，事件重放不会叠第二层。
- 准入点统一标注：[`run.rs`](../../crates/kanzei-app/src/commands/run.rs#L470) `is_engine_round()` 在 `Automatic→Prompt` 归一**之前**判定；[`run.rs`](../../crates/kanzei-app/src/commands/run.rs#L660) 在 `Submission::Prompt` 入库前调用 `attribute_engine_round()`。必须早于队列去重，否则「带前缀文本」与「原始文本」永远匹配不上，会产生重复轮。
- 前端自动续跑闸门不再伪装成用户发言：[`08-compose-runtime.js`](../../crates/kanzei-app/ui/08-compose-runtime.js) 的 `sendAutoToSession` 传 `autoRound: true`；IPC 契约已随 `sync-ipc-contract.mjs` 重新生成（`00-command-contract.js`、`scripts/ipc-commands.json`）。
- 回归：[`engine_gun_is_attributed_to_the_engine_not_the_user`](../../crates/kanzei-app/src/commands/run.rs#L914)。

### 1.2 D-764②：压缩纪要的用户指令清单不收引擎文本

- [`compaction.rs`](../../crates/kanzei-core/src/runner/compaction.rs#L55)：`DIGEST_SYSTEM` 明确「用户指令清单」只收用户本人按键；以「Kanzei 引擎自动续跑」或「Kanzei 系统通知」开头的轮次不得写入，也不得当成对提问/方案的批准。
- 防漂移测试 [`纪要模板逐字认引擎归属前缀`](../../crates/kanzei-core/src/runner/compaction.rs#L777) 把模板文案与 `ENGINE_ROUND_MARKER` 绑在一起：任一边改字，测试红。

### 1.3 D-764③：确认记录必须引用真实作答凭证

- 新模块 [`question_ref.rs`](../../crates/kanzei-harness/src/question_ref.rs)：凭证格式 `question:{run_id}:{call_id}`，签发（runner）与识别（门禁）共用一份实现。
- 作答时签发：[`question.rs`](../../crates/kanzei-core/src/runner/drive/question.rs#L77) 在 `User answer: {答案}` 之后追加一行凭证；后台问答的异步通知同样带凭证（[`question.rs`](../../crates/kanzei-core/src/runner/drive/question.rs#L58)）。
- 门禁收紧：[`validation.rs`](../../crates/kanzei-tools/src/tracker/validation.rs#L171) 不再接受 `question` 子串，只认逐字凭证或「用户明确豁免 + 原话」；拒绝文案给出可照抄的字段形态（R-354）。模型自己的 `dec-…` 决定从此不能充当用户确认。
- 回归：[`tracker.rs`](../../crates/kanzei-tools/src/tracker.rs#L5529) 三种冒写形态（「已通过 question 确认」「question: 用户确认…」「Agent decision dec-…」）全部被拒，真凭证放行。
- UI 侧：[`05-tool-summary.js`](../../crates/kanzei-app/ui/05-tool-summary.js#L582) 原正则 `[\s\S]*` 会把凭证行吞进「用户回答」气泡，改为只取答案行；[`ui-runtime-smoke.mjs`](../../scripts/ui-runtime-smoke.mjs#L11655) 增加两行输出的回归用例。

**与 R-374 的关系**：R-374（[todo]）验收①要求单题结果逐字保持 `User answer: …`。本轮**保留首行逐字格式**，凭证只在第二行；[`question.rs`](../../crates/kanzei-core/src/runner/drive/question.rs#L76) 有注释说明删掉第二行会让门禁失去可引用凭据。R-374 实施时不要顺手删它。

### 1.4 D-766①：设计时效门禁改读 HEAD

- [`check-design-freshness.mjs`](../../scripts/check-design-freshness.mjs#L135) 用 `git ls-tree`/`git show HEAD:…` 取文档与索引，不再读工作副本。此前「文档已提交、索引行还在工作副本」的树能过自己的门禁。
- [`verify-policy.mjs`](../../scripts/verify-policy.mjs#L27) 的 `BUILD_INPUT_PATHS` 加入 `.kanzei/project`（规范 §6 要求项目资产随代码提交）。注意该文件还混有**另一会话**的改动（`AGENTS.md`、`CONTRIBUTING.md`、`crates/**/fixtures/` 分类），提交时需按 hunk 拆分。

### 1.5 D-766②：回归覆盖

- [`verify-policy-smoke.mjs`](../../scripts/verify-policy-smoke.mjs#L145)：真实临时 git 仓库里断言未跟踪的 `.kanzei/project/architecture/README.md` 会被 `assertCleanBuildInputs` 拒绝，且 `inspectBuildInputs().dirty_paths` 精确等于它。
- `check-design-freshness --self-test` 增加「索引行缺失」用例。

### 1.6 前端诚实性

- [`bootstrap.js`](../../crates/kanzei-app/ui/bootstrap.js#L2)：`00-console.js` 移到第一个 import（诊断采集必须最早求值）。
- [`20-lines.js`](../../crates/kanzei-app/ui/20-lines.js#L203)：线路页自动推进勾选/暂停/本轮后停三处 `void setLineAutoState(...)` 改为 await + 失败回报；勾选被引擎拒收时退回真实投影，不再显示一条没在跑的「自动推进」。
- [`08-compose-runtime.js`](../../crates/kanzei-app/ui/08-compose-runtime.js#L1261)：停机收口路径两处 `void` 改为失败上抛提示。**不回滚**本地投影——停机事件本身来自引擎，回滚会是另一种谎。
- [`00-ui-preferences.js`](../../crates/kanzei-app/ui/00-ui-preferences.js#L95)：偏好写后端失败不再空 catch，改为 `console.warn` 进诊断控制台（`00-console.js` 已接管 warn）。**不**加弹窗：D-404 写明「不打断当前操作」是既有取舍，本轮只补留痕。
- 新增 i18n 键 `自动推进状态更新失败`（[`02-i18n.js`](../../crates/kanzei-app/ui/02-i18n.js#L177)）。

### 1.7 对上一轮分析的两处更正

1. `08-compose-runtime.js` 中 `previous.catch(() => {})` / `.finally(...).catch(() => {})` 是**串行队列卫生**，返回给调用方的 promise 照常拒绝；不是吞错。真正的洞在调用方 `void` 掉返回值（即 1.6 修的线路页）。
2. `uiPrefsSave` 的空 catch 是 D-404 的明文取舍，不是遗留 bug；只补诊断留痕。

## 2. 验证记录（2026-10-22 15:40–15:43，工作树如上）

| 命令 | 结果 |
| --- | --- |
| `cargo fmt --all -- --check` | 通过 |
| `cargo clippy -p kanzei-harness -p kanzei-core -p kanzei-tools --all-targets -- -D warnings` | 通过（20.08s） |
| `cargo test -p kanzei-harness -p kanzei-core -p kanzei-tools question` | 5 / 6 / 6 passed |
| `cargo test -p kanzei-tools tracker::` | 101 passed |
| `cargo test -p kanzei-harness auto_run` | 41 passed |
| `cargo test -p kanzei-core compaction` | 23 passed |
| `cargo test -p kanzei-app engine_gun` | 1 passed（`643 filtered out`） |
| `node --experimental-vm-modules scripts/ui-runtime-smoke.mjs` | 通过：139 个 ui 源、3446 次 invoke、11 个视图切换、0 运行时错误 |
| `node scripts/ui-i18n-smoke.mjs` | 通过：3395 key / 671 HTML 文案 / 56 动态契约 |
| `node scripts/ui-lint-smoke.mjs` | 通过：310 文件 |
| `node scripts/check-design-freshness.mjs`（含 `--self-test`） | 通过 |
| `node scripts/verify-policy-smoke.mjs` | 通过 |
| 自查：`npx eslint <本轮 20 个文件>` | 0 error |

**踩过的坑**：`ui-runtime-smoke.mjs` 必须用 `node --experimental-vm-modules` 启动；裸 `node` 会在 `vm.SourceTextModule is not a constructor` 处崩，报错位置（`esmModuleCache.get(...).namespace`）看起来像业务故障，其实是启动参数问题。注册表里 `ui_runtime` 已带此参数，别手敲漏掉。

## 3. 设计约束与已知交互

- **凭证是形状门禁，不是存在性证明**：交互作答记录落在易失的 `.kanzei/runtime/questions/`，按 R-354 不能作为条目账本证据。因此门禁只校验 `question:{run}:{call}` 形态，作用是把「诚实路径变成复制粘贴」并把散文式冒写与模型自决挡在外面；它不承诺能反查那次调用。
- **R-354**：拒绝文案只要求条目标题自身能提供的证据，并给出可抄写法。
- **R-374**：见 §1.3 末段。

## 4. 工作树状态与并发写入（接手前必读）

- 15:40 时 `git status --porcelain` 共 **307 项**（20 项未跟踪），其中只有 20 个文件属于本轮改动（§6 清单）。
- **另一个进程正在同一工作树里写**：`kzapp.exe`（08:02 采样时 PID 73668）运行中；`scripts/ui-runtime-smoke.mjs` 在 15:39:42 仍被改写；08:00–09:40 之间多个 `ui/*.js`、`scripts/ui-*.mjs` 出现非本轮改动（移动端会话选择器 `sessionOptionText`、`/v1/sessions`、预览几何 fixture 等）。这些改动与我的改动**在同一文件里混排**（`scripts/verify-policy.mjs`、`scripts/ui-runtime-smoke.mjs`）。
- 因此：
  - **不要 `git add .`**；需要按 hunk 挑选（`git add -p`）或等另一会话提交后再合。
  - 不要在此状态下跑 `verify.ps1 -Full`：`.kanzei/project` 现在属于构建输入，未提交的 tracker 会让 `assertCleanBuildInputs` 按设计拒签；且外来在途改动可能随时把门禁跑红。
  - 我的每次验证都是「当时那棵树」的结果；上表最后一轮已包含外来改动，全部为绿。

## 5. 待拍板事项

1. **提交**：本轮 20 个文件 + 本报告。提交身份 `kanzei <vraniumzwt@gmail.com>`，不得含 `Co-Authored-By`；push 前需设 `HTTPS_PROXY=http://127.0.0.1:12000`。
2. **conventions 增补（原计划 D-764④）**：把「从用户已审阅设计文档登记且带裁决的条目视为方案已确认，分批实施不需逐批授权」写进 `.kanzei/project/conventions.md`。该文件是受管文档，需走 `cargo run -p kanzei -- …` 的工具通道或用户批准后手改。
3. **摘除已修复的存量条目**：SEC-01/03/07、WEB-01/04/05、RUN-02/03，以及 `问题.MD` 的九条（本轮已逐条核对为已修）。属 tracker 写入，需授权。
4. **12.3 GB 本地 scratch**（`output/audit-first/` 等）：删除不可逆，留给用户。
5. **`verify.ps1 -Full`**：需先提交 `.kanzei/project` 与全部构建输入，且并发会话收尾。

## 6. 本轮改动文件清单

新增：

- `crates/kanzei-harness/src/question_ref.rs`
- `docs/reports/2026-10-22-session-handoff.md`（本文件）

修改（★ = 与外来改动混排，提交需拆 hunk）：

- `crates/kanzei-harness/src/{lib.rs,auto_run.rs}`
- `crates/kanzei-core/src/runner/drive/question.rs`
- `crates/kanzei-core/src/runner/compaction.rs`
- `crates/kanzei-tools/src/tracker.rs`
- `crates/kanzei-tools/src/tracker/validation.rs`
- `crates/kanzei-app/src/commands/run.rs`
- `crates/kanzei-app/ui/{bootstrap.js,00-ui-preferences.js,02-i18n.js,05-tool-summary.js,08-compose-runtime.js,20-lines.js}`
- `crates/kanzei-app/ui/00-command-contract.js`（生成物，`node scripts/sync-ipc-contract.mjs`）★
- `scripts/ipc-commands.json`（生成物）★
- `scripts/check-design-freshness.mjs`
- `scripts/verify-policy.mjs` ★
- `scripts/verify-policy-smoke.mjs`
- `scripts/ui-runtime-smoke.mjs` ★

## 7. 接手顺序建议

1. 确认并发会话已停（`tasklist | grep -i kzapp`，并观察 `git status` 不再变化）。
2. 复核 §2 命令仍全绿；重点看 `ui_lint` 与 `ui_runtime`（外来改动最活跃的两处）。
3. 按 hunk 提交本轮改动；提交信息写「修 D-764…」而非泛化描述。
4. 提交 `.kanzei/project` 下的 tracker 变更（若属于同一关切），再跑 `scripts/verify.ps1 -Full`。
5. 按 §5 决定 conventions、存量条目摘除与 scratch 清理。

## 8. 同日后续修复与状态对账

- **D-764④ 已完成**：通过真实 `ConventionsTool.get/patch`，带 `expected_hash` 更新 `.kanzei/project/conventions.md`，规定用户已审阅设计文档且已有裁决的登记条目在既有范围内分批实施，无需逐批重问。顺带修正该规范中“异常身份立即改写修正并强推”的旧条款，使其与根 `AGENTS.md` 的保留已发布历史规则一致。专用工具写入凭据保存在 `output/session-handoff-repair/conventions-*.log`。
- **修复异步问题的过早凭证**：原实现发送、取消与真实作答均暴露 `question:{run}:{call}`，使未作答问题也能通过形状门禁。现在待答/取消只带 `pending-question:{run}:{call}`，只有 `AskResponse::Answer` 分支签发作答凭证；解析器拒绝从 pending 标识或畸形长 token 中截取凭证。普通作答的 `User answer: …` 首行格式保留。
- **修复豁免关键词绕过**：tracker 不再凭“用户豁免”四个字放行，必须使用明确豁免声明并附非空的引用原话；不同字段保留边界。`quoted_user_text` 共用成对引号解析，修正多组 ASCII 引号漏取。门禁仍按 R-354 校验本条目的留痕形态，不宣称作答存在性或不可伪造证明。
- **D-766 回归已接线**：修复原脚本解析不到实际 `ID 标题 [状态]` 账本标题、中文终态词边界不匹配的问题；固定一次提交身份后读取全部 Git 对象，读取失败直接失败。新增真实临时 Git 仓库测试，覆盖已提交正文/未提交索引，以及工作副本正文、tracker 不能掩盖 HEAD 冲突。`verification-checks.json` 的 `design_freshness` 每次运行都执行这些回归。
- **存量清单已对账**：在 `2026-10-18-kanzei-improvement-backlog.md` 更新 RUN-02/03、SEC-01/03/07、WEB-01/04/05 的源码和回归状态，保留原观察以供追溯；这些编号并非 tracker ID。`问题.MD` 头部列明九项实现缺陷的现有修复，以及 CAS 仍需协作写者遵守 FileLock 的契约边界。Linux 与真实安装版/受限站点的验证不由 Windows 本地回归替代。
- **账本已闭环**：D-764、D-766 通过 `TrackerTool` 写入逐条验收对账，标记 fixed 后仅归档这两项，保留全部原始条目。D-765 的提交纪律不因这次本地代码修复而标成已解决。
- **已执行的定向验证**：新回归先复现失败再修复；question_ref 3 passed、core question 6 passed、tracker 核心语义准入 1 passed、auto_run 41 passed、compaction 23 passed、app engine_gun 1 passed，设计时效真实 Git 自测与当前 HEAD 门禁通过，UI runtime 为 139 源/3446 invoke/11 视图/0 运行时错误。最终统一验证结果及绑定输入见 `output/session-handoff-repair/修复记录.md` 和同目录验证回执，不能沿用 §2 作为新证据。
- 本次交付为工作树代码、规范与账本修复；没有提交、推送、发布或清理本地 scratch。正式发布仍需干净提交上的 `verify.ps1 -Full`。
