# Defects

## D-504 鞭挞配置双真源与 autoRounds 双计数器,四副本靠手工互拷同步 [fixing] (medium)
- 复现: crates/kanzei-app/ui/08-compose.js:1088-1097 lineAutoConfig 活动线读 DOM 复选框、其他线读 processAutoState Map;同状态另存 localStorage(kz-process-auto-state) 与后端 ui_prefs/auto_state_update(:1014-1021,:1057);autoRounds 全局(:4)与 state.auto_rounds(:337,:380) 靠 07-events.js:439/449/465 手工互拷,:1078 切线再读回
- 影响: 四副本两条同步路径,漏一处即显示 0/10 实际下一轮撞上限;历史已翻车两次
- 来源: 2026-08-18 全库勘察(主会话);D-290/D-353 历史翻车点
- 标签: 前端
- 验收: 收敛单一真源(Map/state),DOM 只做投影;切线/后台线/重启回归用例;冒烟覆盖
- 优先级: P2
- 取活依据: engine:无可执行 WIP，按 defect-first 选择队首 D-504
- 进展: 2026-08-25 复核更正：Computer Use 应用枚举确认安装位 C:\Users\kanzei\AppData\Local\kanzei\kzapp.exe 当前存在唯一窗口且正在使用；此前 Get-Process 路径筛选未识别该 Tauri/WebView2 窗口，不能据此执行重启。代码、静态与自动化回归仍已完成；剩余唯一动作是用户空闲后执行真实退出→重启→回读持久化 auto state，本轮不接管用户会话。
- observed_head: c40a3403448d7c6d4aef1d7b52557bf74989ed37
- observed_worktree_hash: fnv1a64:cbf29ce484222325
- recorded_at: 1787602751911
- 阻塞: 
- 对账: 2026-08-20 对账:用户已关闭 kzapp 窗口,阻塞解除;剩余动作=重新启动安装位 kzapp 回读持久化 auto state 完成真实重启验收(桌面窗口空闲期执行,CLI 循环亦可承接);其余验收已通过(8f490d92)
- 停车: 等待用户空闲窗口：Computer Use 已确认安装位 kzapp 当前有唯一使用中窗口；剩余验收=退出→重启→回读持久化 auto state。用户空闲后由 agent 执行。

## D-566 cross-tree 隔离快照无回收且构建产物误报,121 目录 143MB 纯堆积 [fixing] (medium)
- refs: D-395 D-397 R-306
- 复杂度: 中
- 复现: .kanzei/quarantine/ 现存 121 个目录共 143MB(shell-with-log 82、cross-tree 32、bg 7),最早 2026-08-16,无任何回收路径。抽查 cross-tree-1787021081327:内容是 crates/kanzei-app/gen/schemas/desktop-schema.json 与 windows-schema.json——Tauri 构建产物,任一线跑构建即重生成,被越界检测当跨树写入取证;cross-tree-1787060772922 存的是 p16 线自己 R-299 B1 提交(7188ba76)的前置内容,合法自身工作被判越界。08-16 单日 30 次 cross-tree 隔离(两线并行互撞日)
- 影响: 误报占绝大多数,真越界信号被淹没;143MB 取证快照只进不出;构建产物类路径每次并行构建都会再触发
- 标签: 流程
- 验收: ①构建产物路径(gen/schemas 等)进入越界检测豁免清单或按内容指纹放行,有定向测试;②quarantine 提供按日期/类型的清理入口(dry-run+实际释放量),真越界证据可显式保留;③清理后 121 个存量目录处置留痕;④真实并行双线构建实测不再产生 schema 误报
- 优先级: P2
- 进展: B1 已提交 38497890：cross_tree.rs:48-82 新增 gen/schemas 完整路径级豁免，:214-218/:298-300 两条扫描路径共用 is_excluded_path；新增 tools/quarantine.rs:1-202，已知类型/毫秒时间戳扫描、dry-run eligible_bytes、apply freed_bytes、未知命名目录 preserved_paths。B2 已提交 e4ecfcf1：cli/quarantine.rs:1-121 提供真实 `kz quarantine` 调用方，默认 dry-run，apply 必须带 type/date 筛选；cli/mod.rs:20-56 接入分发，:86-90 接入帮助。B2 审计补强已提交 3adcfd66：tools/quarantine.rs:21-51 新增 CleanupAudit，:96-126 append_audit 每次成功 dry-run/apply 追加 `.kanzei/quarantine/cleanup-log.jsonl`，记录 eligible_paths 与 preserved_paths、筛选条件、字节和模式；:213-217 单测断言候选与保留目录均进 JSONL。T-1786922726528：393 passed、0 failed、1 ignored；T-1786922726529：真实 CLI dry-run 生成审计记录。新增真实并行验证：T-1786922726530，主树与同 HEAD 临时 worktree 并行执行 cargo tauri build --config {bundle active=false} 均 exit=0，两边各生成4个 gen/schemas 文件，主树 quarantine 目录数保持1、临时树为0。验收对账：①已完成，路径级豁免与回归在 cross_tree.rs:48-82、:806-828，T-1786922726524；②已完成，真实 CLI 为 cli/mod.rs:52、cli/quarantine.rs:58-91，dry-run/apply证据 T-1786922726527，未知证据保留由 tools/quarantine.rs:82-93、:128-156 保证；③新增未来每次清理的逐目录审计留痕：tools/quarantine.rs:96-126 的 cleanup-log.jsonl，T-1786922726528/6529 已断言候选/保留路径落盘并由真实 CLI 触发；历史基线仍为121目录/143MB，当前盘点为1目录/1204224 bytes，历史120目录无独立逐目录 manifest，故历史存量部分仍显式降级；④已完成，真实并行双线 Tauri 构建 T-1786922726530：两 worktree 同 HEAD、两个真实构建均成功，gen/schemas 各4个且无新增 quarantine schema 误报。下一步：提交本次并行验证记录；D-566 仍 fixing，仅③历史逐目录证据缺口未关闭。
- observed_head: 3adcfd663ccd5736581d107aa09ffa36a3926fd6
- observed_worktree_hash: fnv1a64:441f9460a9730954
- recorded_at: 1787192820656
- 取活依据: engine:无可执行 WIP，按 defect-first 选择队首 D-566(unblocks=0)
- 批次: 2/2
- 停车: 历史121目录逐目录manifest无法从当前文件系统重建；代码与真实并行构建已完成，暂让位下一条可执行缺陷；恢复人:agent，恢复条件:找到历史清单或重新产生可逐目录审计的存量窗口

## D-568 记忆 INDEX 描述串号污染:M-014/M-015 描述抄错条目,毒化 FTS 检索 [fixing] (medium)
- 复杂度: 小
- 复现: .kanzei/memory/INDEX.md:M-014 标题「HTML 静态文案必须登记进资源表」但描述整段是 M-009 的「edit 报 old_string not found 时必读…」;M-015 标题「SSE 流内 context overflow」描述却是 M-029 的「处理 bash git 拦截…结构化工具显式 stage」。index.db 的 memory_fts 索引 description 字段,错配描述使这两条在错误查询下被召回
- 影响: FTS 检索被毒化:错误主题命中错误记忆;INDEX 是每会话注入的真源,串号直接影响召回质量
- 标签: 后端
- 验收: ①M-014/M-015 描述修正与源文件 description 一致;②全量 INDEX 行与对应 M-*.md 的 description 做一次机械一致性核对,输出不一致清单并修复;③重建 index.db FTS 后检索抽查不再串号;④INDEX 生成/更新路径补一致性断言防复发
- 优先级: P2
- 取活依据: engine:唯一可执行 WIP 是 D-568，必须先恢复它
- 进展: 对账 2026-08-20(resume reconcile):④已落地——7c238573(D-590)在 store.rs assert_index_matches_entries 接入 refresh_derived 写入路径+守护测试 index_description_guard_rejects_mismatched_source,验收④视为既有能力核销。①②③未落地,且发现比登记更深:不止 INDEX 串号,M-014/M-015 源文件本身 description+正文整段串号(当前 M-014 正文是 M-009 的 edit SOP、M-015 正文是 M-029 的 git 拦截 SOP),真源=git 1476098e 建条原始版(已从历史取出全文)。修正路径被 managed fence 挡死:.kanzei/memory/*.md 仅 memory 写工具白名单可写,edit 被拒(R-316 来源实录);同步修正通道 R-316 仍 todo。下一步:探查既有 memory 工具族是否已有改现有条目文本的能力,无则按 R-316 最小实现(memory 文本修正工具+fence 白名单+审计留痕),落地后修 M-014/M-015 源文件→refresh_derived 重建 INDEX+FTS→②全量机械核对→③FTS 抽查。
- observed_head: 11b60ae32647a5ff999329120316e8ffebad7fd8
- observed_worktree_hash: fnv1a64:441f9460a9730954
- recorded_at: 1787203506741
- 停车: 停车: 本轮用户请求仅为复核并清理过期阻塞，未进入 D-568 实现；R-316 前置已完成，待后续按 defect-first 恢复 D-568 修复。恢复人:agent。

## D-577 raw_lines 把空行判成游离段落且 raw_delete 报成功后游离行仍在,后置条件不成立 [fixing] (medium)
- 复杂度: 中
- 复现: 两处独立复现。①文章获取器测试项目(2026-08-20):R-002 raw_lines 报 1 条「(空行)」游离行,轨迹显示 raw_delete 返回「已删除第 1 条游离行」后再查仍在;D-001 据此登记并带着未复核的后置条件(进展自写「复核应确认 raw_lines 为空」)归档 fixed,本会话复查游离行依旧在。②kanzei 主库当场复现:R-310/R-311 均为本日 kz CLI req add 正常登记(多 --field 路径),raw_lines 各报 1 条「(空行)」;同日同路径登记的 R-313 却没有——正常登记/更新路径自身就会产生该「游离段落」,与「历史多行写法/手改残留」的工具自述不符,基本可定性检测把序列化产物空行误判为不可寻址内容
- 影响: 工具返回语义误导 agent:报成功但后置条件不成立,弱模型陷入 raw_delete 循环并把未验证的 fixed 写进归档;纯空行本不该被判为不可寻址游离段落;产生元数据治理执行噪音,消耗轮次
- 来源: 2026-08-20 实测复现 + self-found implementation follow-up
- 标签: 核心
- 验收: ①定性空行游离判定是否误报,若误报则空行不再计为游离段落;②raw_delete 返回前复查后置条件,删不掉如实报错而非报成功;③文章获取器 R-002 现场复核游离行清零;④回归测试覆盖「删除报成功后仍存在」形态
- 优先级: P2
- 取活依据: engine:无可执行 WIP，按 defect-first 选择队首 D-577(unblocks=0)
- 进展: 批次: 1/1；已完成：`crates/kanzei-memory/src/docstore/validation.rs:265-287` 过滤纯空白 Raw，`:313-339` 让 raw_delete ordinal 与 raw_lines 同口径，`:346-365` 写回后重新 load 并核对条目存在及非空 Raw 数量；`crates/kanzei-memory/src/docstore.rs:321-331` 与 `crates/kanzei-tools/src/tracker.rs:1518-1528、1616-1626` 加入空行误判回归夹具。关键决策：布局空行不再属于游离段落；raw_delete 失败后置条件返回 error，不报成功。T-1786922726559、T-1786922726560 已通过。条款③仍待外部项目现场复核，不能冒充完成。
- observed_head: cd3b43ecb78444ac519e825e246445f2187b13a1
- observed_worktree_hash: fnv1a64:e3e760efa1f03e67
- recorded_at: 1787235788581
- 验收对账: ①已完成：`crates/kanzei-memory/src/docstore/validation.rs:273-287` 只返回非空 Raw；docstore 回归 `docstore.rs:321-342` 与 tracker 回归 `tracker.rs:1539-1549` 证明布局空行不再计数；T-1786922726559。②已完成：`validation.rs:313-365` 删除按同一 ordinal 契约定位，原子写回后 `load()` + `raw_lines()` 复查条目存在和数量，失败返回“raw_delete 后置条件失败”；T-1786922726559、T-1786922726560。③验收降级：原文“文章获取器 R-002 现场复核游离行清零”本轮未执行，当前仓库无该外部项目与可重放目标命令；实际已由同形态端到端回归 `tracker.rs:1518-1579` 覆盖，外部现场仍需用户/外部项目执行。④已完成：空行在 ordinal 1 时旧实现会误删空行而保留真实游离文本，新回归夹具 `docstore.rs:321-389`、`tracker.rs:1518-1598` 覆盖该“报成功后仍存在”形态；T-1786922726559。
- 阻塞: 文章获取器 R-002 外部现场复核需要用户提供该项目路径或可重放目标命令；解除人:用户提供可访问项目与目标命令后由 agent 执行并回填现场证据。

## D-592 上下文预算检查信 bytes/4 估算不锚定真实 usage,本地小窗口模型压缩零触发直至撞 400 [fixing] (high)
- refs: D-203 D-206 R-219 R-236
- 复现: 2026-08-20 现场:llama-local(qwen3.8-27b,llama-server n_ctx=65536)鞭挞 D-568 任务,真实请求 69889 tokens 撞 provider 400(exceed_context_size_error),全程主动压缩零触发。判定链 context_budget.rs:51 用 bytes/4 估算(context.rs:130)×校准因子与触发线比大小,三重系统性偏低叠加:①bytes/4 对中文(UTF-8 3字节/字实际≈1~1.5 token/字)、代码、llama.cpp jinja 模板渲染的工具 schema 膨胀,合计偏低>2.1×(69889 真实 vs 触发线 32768 未达);②校准单步比值 clamp [0.5,2.0](context.rs:165),系统性偏差≥2× 时数学上限封死追不上,EMA 0.7/0.3 收敛慢且每 run 重置 1.0(assembly.rs:195),恢复大历史的新对话首步最脆;③compaction_budget=limit−max(max_tokens,buffer)(context.rs:92),全局 max_tokens=32768 吃掉 65536 窗口一半。usage 回读链路本身是通的(openai.rs:110 include_usage,drive.rs:609 拿真实 prompt_tokens),但只喂校准 EMA,预算比大小不直接用——真实值在手边,决策看估算
- 影响: 本地小窗口 provider 跑长任务必然在压缩触发前撞墙 400,自主推进直接致命中断;窗口越小、内容越偏中文/代码,撞墙越早;98304 窗口同样防不住(偏差>1.5× 即穿)
- 来源: 2026-08-20 用户实测反馈『快摸到上限了还是没压缩』,主会话诊断
- 标签: 核心
- 边界: 历史侧 Part::Reasoning 剪枝(openai 协议 build_body openai.rs:83-91 从不回传,drive.rs:582 却存进历史虚增估算)只能与本条①同批落地——单独剪会让估算更小、压缩触发更晚,加重症状;Qwen 官方口径『多轮剥离 thinking 但多步工具调用期间保留』的质量权衡(llama-server --reasoning-preserve)不在本条,另行评估
- 验收: ①预算检查锚定上一步真实 prompt_tokens(last_input_tokens 已在手)+本步新增内容估算增量,bytes/4 全量估算只做冷启动兜底;②校准按 provider 持久化或冷启动用保守初值,消除每 run 重置 1.0 的首步裸奔;③compaction_budget 对小窗口自适应,max_tokens 不得吃掉固定一半窗口;④回归:模拟估算偏低 2 倍场景压缩在撞墙前触发;⑤llama-local 真实长任务(多步工具循环读大文件)实测不再 400
- 优先级: P1
- 批次: 3/3
- 批次表: B1/3：核对预算决策与 usage 调用链，落地真实 prompt_tokens 锚定及回归；B2/3：落地 provider 校准持久化/保守冷启动与小窗口 compaction_budget；B3/3：修正 Reasoning 历史估算一致性、跑全链路验证并收口。
- 进展: B3 已落地并提交 `2871fee7`（D-592 B3 修正协议感知上下文预算），提交文件与预期一致。实现与证据逐条对账：① `crates/kanzei-core/src/runner/drive/context_budget.rs:54-61` 用 `last_input_tokens + max(current_estimated - last_estimated, 0)` 锚定上一步真实 usage，冷启动才走完整估算×校准；`drive.rs:500-507` 用实际 `route.kind` 记录与 wire 请求一致的原始估算；B1 回归 `T-1786922726566`。② `context.rs:16-21` 提供保守冷启动校准 2.0，`drive/assembly.rs:195-198` 消费该值，`T-1786922726568` 通过；本条已明确采用冷启动保守初值，不新增 provider 持久化真源。③ `context.rs:99-107` 将 max_tokens 与 buffer reserve 各限制在 context_limit/3，保留 context_limit/4 封底，`T-1786922726568` 覆盖 16k/32k/65k。④ `context.rs:125-155` 与 `drive/context_budget.rs:54-61,98-107` 按 ProtocolKind 对齐估算、预算检查和 trim_tail；OpenAI Chat 不计实际 builder 丢弃的 Reasoning，Responses 保留；回归 `context.rs:530-551`、`T-1786922726569`（223 passed），并由 B1 的 `T-1786922726566` 覆盖低估增量触发链。⑤ 验收降级：原文要求 llama-local 真实长任务多步工具循环不再 400→本批未执行真实 provider 长任务，原因是当前环境没有可安全接管的用户 llama-local 实测窗口；代码级预算链已验证，但该现场证据仍由用户/后续真实窗口执行。workspace 门禁 `T-1786922726570`：fmt/check 通过，clippy 被 D-603 `manager.rs:644,653` 的既有 &PathBuf 问题阻断；D-592 代码未混入该无关修复。下一步：由用户提供或释放可安全接管的 llama-local 实测窗口，执行多步工具循环读大文件并记录真实请求不再 400；在此之前保持 fixing。
- observed_head: 2871fee76493998dda6871a50059918849ac3826
- observed_worktree_hash: fnv1a64:abf42289ad631ab3
- recorded_at: 1787239449856
- 取活依据: engine:唯一可执行 WIP 是 D-592，必须先恢复它
- 阻塞: 等待用户提供或释放可安全接管的 llama-local 实测窗口；解除人：用户准备 llama-server 窗口并告知 agent 执行多步工具循环验收。

## D-655 轮末统计切片 prior.len() 在轮中压缩后错位,episodes/harvest/失败提炼画像失真 [fixing] (medium)
- 复现: 与 D-654 同根因的统计侧残留:coordinator.rs 轮末 summarize_tools 切片、persistence.rs:133 与 crates/kanzei/src/cli/run/finalize.rs:151 的 this_run 切片都按 prior.len().min(len) 取本轮;compact_with_digest/trim_tail_for_protocol 在轮中结构性删短 messages 后切片错位:压缩量大于本轮新增时切空,否则混入错误区段。鞭挞判定已在 D-654 改事件真源,这三处统计口径(episode metrics/harvest_end_of_run 失败提炼/工具计数)仍用切片
- 影响: 遥测与记忆收割质量:压缩触发的轮次 episode 画像不全或为空、失败观察漏投;不影响鞭挞判定(已解耦)
- 来源: D-654 修复现场 self-found
- 标签: 后端
- 验收: ①本轮口径不再依赖 prior.len() 盲切:由 runner 维护跨压缩稳定的本轮边界(RunSummary 携带)或统计改事件真源;②三处调用点(coordinator/persistence/CLI finalize)统一新口径;③回归:模拟轮中压缩删短 prior 后,episode tools/metrics 与 harvest 仍只含本轮内容
- refs: D-654
- 优先级: P2
- 进展: 2026-08-25 对账更正：D-655 实现与三 crate 定向回归已完成，D-666 已归档 fixed，cargo fmt --all -- --check 当前通过。尝试关闭时被 D-664 门禁拒绝：当前 HEAD 尚无绑定该条目的 full verify 全绿证据；不能绕过关闭门禁，也不把实现完成误写成条目已关闭。后续只需在不覆盖用户未提交改动的前提下取得当前 HEAD verify 证据，再执行 close/archive。
- observed_head: c40a3403448d7c6d4aef1d7b52557bf74989ed37
- observed_worktree_hash: fnv1a64:cbf29ce484222325
- recorded_at: 1787602813016
- 取活依据: engine:唯一可执行 WIP 是 D-655，必须先恢复它
- 停车: 
- 阻塞: 等待 D-664 关闭门禁所需的当前 HEAD 绑定 full verify 全绿证据；当前工作树含用户未提交的 memory 改动，本轮不覆盖或擅自提交。取得独立、可审计的 verify 证据后再关闭。

## D-662 托管文档专用工具膨胀致工具选择面过载 [fixing] (medium)
- 原始描述: 外部评估 #5：Managed Documents 造成 Tool Explosion，从 Unix-like tools 走向 Domain-specific OS。用户判定这是工具设计问题，算缺陷不算决策
- 复现: 当前注册工具已 30+，其中 req/defect/idea/decision/architecture/test_record/work/memory_* 等托管域工具与通用 edit/write 语义重叠；模型需在 edit 与 req(update) 之间做领域判断，工具越多误选概率越高，且每个工具签名等同公开 API
- 标签: 流程
- 优先级: P2
- 进展: 第一步做「量」不做「并」:工具面预算门禁 profiles.rs::tool_surface_budget(dev/readonly 各一条 + 记忆写路径护栏),预算取实测值不留余量。第二步按写读分离减面:2026-08-21 摘除 todowrite——它与 tracker 的 批次/进展 是同一件事的两个真源,而后者持久(过夜断了也接得上),且 dev 提示词本就写着「批次单元格是进度从外部唯一可见的地方」;实测 1019 轮里 todowrite 只用 57 次而 req+defect 用了 2378 次。dev 工具面 30 → 29,连带摘除 #todo-panel 面板/CSS/i18n/D-350 冒烟段(工具一摘没人能填它,留着就是死代码)。剩余:tracker 四件套与 research 五件套的减面按同一原则(挪进流程专用子代理快照)另行评估
- observed_head: 81a80c64d552d4da9aba0f5692c23d2b5bafb012
- observed_worktree_hash: fnv1a64:a1d1426a5522a197
- recorded_at: 1787288788389
- 停车: 本轮 WIP 超限，工具面预算门禁已落地但后续减面尚未形成可执行批次；先让位当前缺陷优先项 D-655，槽位释放后按取活顺序恢复。恢复人:agent

## D-712 800px 窄窗口侧栏抽屉遮挡对话空态,首屏引导被截断 [open] (high)
- 复杂度: small
- 复现: 在静态 UI 页面使用真实浏览器视口 800x600 并展开左侧栏：crates/kanzei-app/ui/style.css:764-769 将非折叠 sidebar 设为 absolute 抽屉，而空态仍在主内容区居中；截图显示空态图标和引导文字被侧栏覆盖/截断。1024x768 基本正常，说明问题集中在窄窗口断点。
- 影响: 窄屏首次进入对话时，空态引导、图标与文案不可完整阅读，侧栏与主内容争抢首屏空间；用户无法清楚理解下一步操作。
- 来源: 2026-08-25 全面项目审查；Playwright 静态浏览器 800x600/1024x768 视觉检查；docs/design/reliability_usability_self_hosting_quality.md:284-292 窄窗口可读性基线
- 标签: 前端
- 验收: 真实 UI 在 800x500、800x600、1024x720、1280x840 且侧栏展开/折叠时，空态图标文案、prompt/composer、右侧抽屉和状态栏均不重叠不截断；键盘焦点始终可见；补充对应窄窗口回归截图或自动化断言。
- 优先级: P1

## D-713 需求标题状态与正文状态冲突,取活和恢复口径不唯一 [open] (high)
- 复杂度: medium
- 复现: .kanzei/project/requirements.md 中存在结构化状态与正文状态不一致：R-284:23 标题为 [doing]、:34 正文状态为 todo；R-249:117/133 同样为 doing/todo；R-264:135/152 同样为 doing/todo；R-101:71 标题为 [doing]，:86 又记录 doing→todo 的状态纠正。机械对账已复现上述冲突。
- 影响: work next、WIP 统计、依赖判断和 Agent 恢复可能按不同字段选取同一需求，导致已暂停条目被误取活、进行中条目被漏掉，需求台账无法作为唯一进度真源。
- 来源: 2026-08-25 全面项目审查；requirements.md 状态一致性对账
- 标签: 流程
- 验收: 明确唯一规范状态源并完成存量清理；正文不再保留会被解析为当前状态的重复字段，历史变更移入进展/对账；机械检查对状态冲突直接失败；kz work next、需求页面和 Agent 恢复读取同一状态口径，并补充 R-284/R-249/R-264/R-101 回归核对。
- 优先级: P1
