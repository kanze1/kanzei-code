# D3：上下文、读凭据、工具结果与权限接口

基线 `f4314972380a7f7e447ef04306427ee20acf51f1`，既有A树、分支 `kanzei/audit-d3-permission-context`。root已复制并校验D2证据；D3未带root M5 dirty。C5正式释放后root独占Cargo槽交D3，按指定范围完成验证；未跑全workspace/native/UI。

## Scope / coverage

恰5个目标全文（包含文件内测试），已按上下文/读账本→工具合同→权限顺序读完：

- `crates/kanzei-harness/src/lib.rs`
- `crates/kanzei-harness/src/context.rs`
- `crates/kanzei-harness/src/read_ledger.rs`
- `crates/kanzei-harness/src/tool.rs`
- `crates/kanzei-harness/src/permission.rs`

基线coverage.json五个均not_reviewed，既有parallel报告没有这五个的已完成全文项。必要caller与harness/runner切片不计全文；公共coverage/inventory/queue未改。lib/context/read_ledger/tool均PASS不改，只有permission.rs修一条P1。无新依赖/framework/schema。

## 依赖与状态owner

base路径/身份原语→ToolCtx项目根/树/写仲裁key；ReadLedger由conversation/session runtime持Arc<Mutex<HashMap<PathBuf,hash>>>，clone共享、child独立。ToolCtx携带run/process/session身份、mailbox/inbox/ledger；读工具record版本，write/edit核对凭据，不能把会话共享当操作CAS。

context FnSource只保存key/render/refresh标记，无持久写入；HarnessDraft注册ContextSource，resolve/materialize生成不可变snapshot，runner按refresh_each_step临时请求system渲染。

Ruleset保存普通last-match-wins规则、hard_denies和managed guidance；HarnessSnapshot持immutable权限快照，core serial/parallel resources gate评估；session授权/always持久写链已有M3，不扩本包。ToolOutput区分provider is_error与机器outcome/code/display/images/artifact，runner/model/UI/telemetry分别消费，不从单一红色状态推断产品故障。

## caller证据索引

完整repo搜索切片索引保存在ignored output/parallel-D3/{context,read-ledger,tool-context,tool-output,permission}-callers.txt，补充permission-public-callers.txt、tool-output-public-callers.txt。索引含测试/注释/同名命中，并不把这些计成生产caller或已审全文。公开构造、builder、trait入口、权限读写及路径函数按当前生产链核实。

- context：tools/base、profiles/dev/general、research_workflow/tool、work/context、team/tools、harness/markdown注册；harness/harness resolve+materialize与core runner消费。
- ReadLedger：CLI run、app SessionRuntime+run coordinator、tools schedules/team、core subagent创建或隔离；tools/read记录；tools/write/edit读取/更新；read_receipt_tests合同切片。
- ToolCtx：CLI/app实际组装、core child派生、tools bash/worktree/managed/concurrency；Tool trait实现与tool_pipeline调用；全部引用见索引。
- ToolOutput：tools/memory工具结果→tool_pipeline→core serial/parallel drive与events/model/UI/telemetry；构造/消费者索引完备，动态trait入口需人工检查。
- permission：config/profile组装Ruleset→harness snapshot/materialize→core drive/permissions及parallel_tools；public路径规范化也供read/write/edit/bash/file_checkpoints，bash JSON保持opaque、路径资源走既有normalize。

## public API 与实际边界

- context：source/refreshing_source→FnSource实现ContextSource；tools/base.rs110/127、profiles/dev.rs202/237/275/360/381/406、profiles/general.rs35、profiles.rs110、research_workflow/tool.rs126、work/context.rs71、team/tools.rs50/61、app/collaboration.rs453、harness/markdown.rs29注册。harness.rs236/239选择并渲染，core runner/drive/assembly.rs90/92和drive.rs247初始/逐步消费。Option空内容沿既有省略合同，无新持久owner。
- ReadLedger：record/expected/forget/clear全调用索引见read-ledger-callers；tools/read.rs102起记录真实已读字节哈希，tools/write.rs23/24核对旧凭据、成功写入后更新，edit/insert复用此入口。app/state.rs138/554持会话账本，run/coordinator.rs76克隆，conversation.rs58和conversation_actions.rs167清空；CLI/run.rs223与schedules/executor.rs397创建，core/subagent.rs514和team/mod.rs1473为child隔离。账本不是磁盘CAS，其他已有写原语承担写入边界；未发现该下层合同新错误。
- ToolCtx：Default/new/discovering及identity/work_priority/session_id/read_ledger builder不改变资源策略。显式cwd/project_root由CLI、app、team、schedule与subagent组装；discovering实际生产调用为app/docs.rs576。ToolConcurrency的worktree键/项目写键供core/tool_exec.rs89/785和drive/parallel_tools.rs163；Tool resources_with_ctx供serial权限与parallel权限，不能用并发键代替授权资源。
- ToolOutput：ok/error/noop/needs_correction/needs_confirmation/failed、model_content、display/images由工具构造并经tool_pipeline/core实际消费；core/tool_exec.rs693与drive/task_results.rs11/21回喂模型。repair_hint实际入口为tools/lib.rs137解析输入、core/drive/serial_tools.rs179解析失败。is_error保留provider合同，expected rejection不能直接计作Failed；未修改结果合同。
- Ruleset：push/extend由Base/Profile/Markdown/ConfigComponent贡献普通规则；push_hard_deny/push_managed_hard_deny由profile/托管写边界贡献，app/run/assembly/components.rs85用push_denial_note。managed_resources供harness.rs62检查提示工具，managed_for供harness.rs280拒绝指引，retain_available_tool_hints实际生产是profiles/general.rs27；rules()当前没有外部生产调用。evaluate/evaluate_with_rule由harness.rs252/261包装，再由core/drive/permissions.rs47、parallel_tools.rs58/141/144评估每个资源。resource_match_for_action还供core两门禁的本轮允许规则及CLI/mod.rs280 allow_once；is_structured_bash_resource供config/permissions.rs79/128规则分类。路径函数还被文件工具/checkpoints调用；bash_prefix_match仅Ruleset内部及测试，当前structured bash JSON不经过旧字符串前缀授权。

## P1 根因与最小修复

唯一根因：action_fully_denied把字面资源`*`的Deny当成整个action没有例外。合法有序配置`write/* Deny → write/README.md Allow或Ask`在evaluate上允许/询问README.md，但模型工具列表移除write，合法操作无入口。research profile也有全局Deny后资源Allow的实际形状（research.rs156–175），不另计根因。

完整直接生产caller只有harness.rs152（materialize_tools）、167（deferred loader是否启用）、175（is_deferred）、361（permission_snapshot_of）；core/drive/assembly.rs81/83用物化工具和resident specs装配真实模型调用，deferred catalog沿同一过滤链。harness.rs为必要caller切片，不计本包新全文。

修复保持公共签名：harddeny对action匹配且resource为全`*`时整体拒绝；普通规则按原逆序扫描，后置Allow/Ask潜在资源例外保留工具，最后覆盖全`*`的Deny才整体拒绝。遇不能证明全拒绝的资源模式保守保留。evaluate/evaluate_with_rule、规范化、harddeny执行优先级和真实资源门禁不变；不枚举资源或新增模式包含证明框架。

两条真实Harness+ConfigComponent回归均在permission.rs：`configured_resource_exceptions_keep_tools_visible_and_runtime_policy_exact`覆盖Allow/Ask及resident/deferred，断言README例外可见、其他/private资源仍Deny且permission.effect仍为字面*的Deny；`whole_action_denials_keep_normal_last_match_and_hard_priority`覆盖hard全*、最后globalDeny仍摘除。注册测试Tool和真实ToolSearchTool仅用于装配，不执行文件/模型操作。root已审核实际caller合同并批准该最小边界。

## 历史决定与排除项

docs/design/harness_m1.md55及docs/reports/2026-10-02-code-map/02-runtime.md80–86说明普通last-match、harddeny优先、工具动态加载不放宽权限。docs/design/cc_codex_alignment_impl_maps.md1099–1218的R-367给出真实读凭据、子账本隔离和失败语义；历史设计草案不是当前未实现证据。ToolOutcome历史审计只用作分类提示，判断按当前source/consumer核实。

没有把缺测试、路径别名极端假设、纯字符串bash旧接口、潜在callback panic计成新bug；必要caller未扩大成全文coverage。

## 验证状态

五个全文完成；两条真实Harness回归各1 PASS。ignored `output/parallel-D3/negative.py`只恢复旧method逻辑并保留真实测试支架：例外回归实际断言失败（exit101，非编译失败），旧method安全控制仍1 PASS；finally原字节恢复，前后SHA均`20b1d50da9ad6a5ce8ea0104e679a0d81d105751e6eb04e849d5354a815d470e`。

进入槽前及恢复源码后刷新所有非vendor/binaries Rust mtime；恢复后的full harness 213 PASS、0 FAIL、0 ignored，doctest 0。harness/core/tools all-targets check、Clippy -D warnings、三package fmt和git diff --check全部exit0。真实测试使用自建D3 HOME/USERPROFILE/KANZEI_HOME/AppData及已初始化隔离D1 Cargo/Rustup缓存，无真实用户配置/凭据；不将编译check计作core/tools运行测试。全workspace/native/UI由root最终整合承担。raw/LF SHA及精确ignored evidence见D3-verification.json；M6-read-map.md只是下一包只读准备，不纳入D3测试/修复证据。
