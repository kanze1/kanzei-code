# Defects

## D-504 鞭挞配置双真源与 autoRounds 双计数器,四副本靠手工互拷同步 [fixing] (medium)
- 复现: crates/kanzei-app/ui/08-compose.js:1088-1097 lineAutoConfig 活动线读 DOM 复选框、其他线读 processAutoState Map;同状态另存 localStorage(kz-process-auto-state) 与后端 ui_prefs/auto_state_update(:1014-1021,:1057);autoRounds 全局(:4)与 state.auto_rounds(:337,:380) 靠 07-events.js:439/449/465 手工互拷,:1078 切线再读回
- 影响: 四副本两条同步路径,漏一处即显示 0/10 实际下一轮撞上限;历史已翻车两次
- 来源: 2026-08-18 全库勘察(主会话);D-290/D-353 历史翻车点
- 标签: 前端
- 验收: 收敛单一真源(Map/state),DOM 只做投影;切线/后台线/重启回归用例;冒烟覆盖
- 优先级: P2
- 进展: 验收对账：①单一真源(Map/state)、DOM 仅投影：既有实现 crates/kanzei-app/ui/08-compose-runtime.js:899-1004、08-auto.js:26-34；运行时回归 T-1786922726973 覆盖 lineAutoConfig 不读 DOM。②切线/后台线隔离：既有实现 08-compose-runtime.js:1006-1054、07-events.js:546-635；T-1786922726973 覆盖后台双线路连续轮次、线路级配置与停机同步。③重启回归：安装位 C:\Users\kanzei\AppData\Local\kanzei\kzapp.exe 的真实 UIA 冷启动/ValuePattern 回读/需求缺陷→对话往返通过 T-1786922726971，但当前窗口非本次脚本所有，未执行退出→重启→回读持久化状态，待用户空闲窗口。④冒烟覆盖：T-1786922726972（29 个 UI JS 语法 + 57 文件 ESLint）与 T-1786922726973（29 个 UI JS、2641 次 invoke、10 视图、0 运行时错误）。上述源码能力为既有实现，本轮仅完成对账与验证；下一步由用户空闲后提供可关闭窗口，再执行真实重启回归。
- observed_head: 4a85596cbcb5f8a4fe056f11b741317a14216f75
- observed_worktree_hash: fnv1a64:30acc4843d86176e
- recorded_at: 1788658691867
- 阻塞: 用户：在 kzapp 安装位窗口空闲后关闭并允许 agent 执行一次退出→重启→回读持久化 auto state；当前 UIA 已识别窗口非本次测试所有，agent 不接管或强行关闭用户会话。解除条件:用户
- 对账: 2026-09-05 对账:kzapp 安装位进程当前未运行(Get-Process kzapp 为空),停车前提「等待用户空闲窗口」已达成,恢复为 defect-first 队首 WIP;剩余动作=启动安装位 kzapp 回读持久化 auto state 完成真实重启验收
- 停车: 

## D-568 记忆 INDEX 描述串号污染:M-014/M-015 描述抄错条目,毒化 FTS 检索 [fixing] (medium)
- 复杂度: 小
- 复现: .kanzei/memory/INDEX.md:M-014 标题「HTML 静态文案必须登记进资源表」但描述整段是 M-009 的「edit 报 old_string not found 时必读…」;M-015 标题「SSE 流内 context overflow」描述却是 M-029 的「处理 bash git 拦截…结构化工具显式 stage」。index.db 的 memory_fts 索引 description 字段,错配描述使这两条在错误查询下被召回
- 影响: FTS 检索被毒化:错误主题命中错误记忆;INDEX 是每会话注入的真源,串号直接影响召回质量
- 标签: 后端
- 验收: ①M-014/M-015 描述修正与源文件 description 一致;②全量 INDEX 行与对应 M-*.md 的 description 做一次机械一致性核对,输出不一致清单并修复;③重建 index.db FTS 后检索抽查不再串号;④INDEX 生成/更新路径补一致性断言防复发
- 优先级: P2
- 进展: 对账 2026-08-20(resume reconcile):④已落地——7c238573(D-590)在 store.rs assert_index_matches_entries 接入 refresh_derived 写入路径+守护测试 index_description_guard_rejects_mismatched_source,验收④视为既有能力核销。①②③未落地,且发现比登记更深:不止 INDEX 串号,M-014/M-015 源文件本身 description+正文整段串号(当前 M-014 正文是 M-009 的 edit SOP、M-015 正文是 M-029 的 git 拦截 SOP),真源=git 1476098e 建条原始版(已从历史取出全文)。修正路径被 managed fence 挡死:.kanzei/memory/*.md 仅 memory 写工具白名单可写,edit 被拒(R-316 来源实录);同步修正通道 R-316 仍 todo。下一步:探查既有 memory 工具族是否已有改现有条目文本的能力,无则按 R-316 最小实现(memory 文本修正工具+fence 白名单+审计留痕),落地后修 M-014/M-015 源文件→refresh_derived 重建 INDEX+FTS→②全量机械核对→③FTS 抽查。
- observed_head: 11b60ae32647a5ff999329120316e8ffebad7fd8
- observed_worktree_hash: fnv1a64:441f9460a9730954
- recorded_at: 1787203506741
- 停车: 排队:D-504 现为 defect-first 队首 WIP,本条排其后恢复;R-316 修正通道已 done,恢复后按进展所列路径修 M-014/M-015 源文件→refresh_derived 重建 INDEX/FTS→②全量核对→③抽查;恢复人:agent;解除条件:D-504

## D-577 raw_lines 把空行判成游离段落且 raw_delete 报成功后游离行仍在,后置条件不成立 [fixing] (medium)
- 复杂度: 中
- 复现: 两处独立复现。①文章获取器测试项目(2026-08-20):R-002 raw_lines 报 1 条「(空行)」游离行,轨迹显示 raw_delete 返回「已删除第 1 条游离行」后再查仍在;D-001 据此登记并带着未复核的后置条件(进展自写「复核应确认 raw_lines 为空」)归档 fixed,本会话复查游离行依旧在。②kanzei 主库当场复现:R-310/R-311 均为本日 kz CLI req add 正常登记(多 --field 路径),raw_lines 各报 1 条「(空行)」;同日同路径登记的 R-313 却没有——正常登记/更新路径自身就会产生该「游离段落」,与「历史多行写法/手改残留」的工具自述不符,基本可定性检测把序列化产物空行误判为不可寻址内容
- 影响: 工具返回语义误导 agent:报成功但后置条件不成立,弱模型陷入 raw_delete 循环并把未验证的 fixed 写进归档;纯空行本不该被判为不可寻址游离段落;产生元数据治理执行噪音,消耗轮次
- 来源: 2026-08-20 实测复现 + self-found implementation follow-up
- 标签: 核心
- 验收: ①定性空行游离判定是否误报,若误报则空行不再计为游离段落;②raw_delete 返回前复查后置条件,删不掉如实报错而非报成功;③文章获取器 R-002 现场复核游离行清零;④回归测试覆盖「删除报成功后仍存在」形态
- 优先级: P2
- 进展: 批次: 1/1；已完成：`crates/kanzei-memory/src/docstore/validation.rs:265-287` 过滤纯空白 Raw，`:313-339` 让 raw_delete ordinal 与 raw_lines 同口径，`:346-365` 写回后重新 load 并核对条目存在及非空 Raw 数量；`crates/kanzei-memory/src/docstore.rs:321-331` 与 `crates/kanzei-tools/src/tracker.rs:1518-1528、1616-1626` 加入空行误判回归夹具。关键决策：布局空行不再属于游离段落；raw_delete 失败后置条件返回 error，不报成功。T-1786922726559、T-1786922726560 已通过。条款③仍待外部项目现场复核，不能冒充完成。
- observed_head: cd3b43ecb78444ac519e825e246445f2187b13a1
- observed_worktree_hash: fnv1a64:e3e760efa1f03e67
- recorded_at: 1787235788581
- 验收对账: ①已完成：`crates/kanzei-memory/src/docstore/validation.rs:273-287` 只返回非空 Raw；docstore 回归 `docstore.rs:321-342` 与 tracker 回归 `tracker.rs:1539-1549` 证明布局空行不再计数；T-1786922726559。②已完成：`validation.rs:313-365` 删除按同一 ordinal 契约定位，原子写回后 `load()` + `raw_lines()` 复查条目存在和数量，失败返回“raw_delete 后置条件失败”；T-1786922726559、T-1786922726560。③验收降级：原文“文章获取器 R-002 现场复核游离行清零”本轮未执行，当前仓库无该外部项目与可重放目标命令；实际已由同形态端到端回归 `tracker.rs:1518-1579` 覆盖，外部现场仍需用户/外部项目执行。④已完成：空行在 ordinal 1 时旧实现会误删空行而保留真实游离文本，新回归夹具 `docstore.rs:321-389`、`tracker.rs:1518-1598` 覆盖该“报成功后仍存在”形态；T-1786922726559。
- 阻塞: 
- 停车: ①②④已完成并有回归;③需在外部项目「文章获取器」现场复核 R-002 游离行清零,本机 Documents 下未找到该项目,agent 无法自行执行;需用户指明项目位置,或接受③按同形态回归(tracker.rs:1518-1579)降级后关闭;解除人:用户;解除条件:用户

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
- 阻塞: 
- 停车: ①～④代码与回归已完成;⑤需 llama-local(llama-server + Qwen3.8-27B)真实多步工具循环长任务实测不再 400,当前本机只有 ollama 进程、PATH 无 llama-server,agent 无可接管的实测窗口;需用户启动 llama-local 并允许 agent 接管一次长任务,或接受⑤降级后关闭;解除人:用户;解除条件:用户

## D-662 托管文档专用工具膨胀致工具选择面过载 [fixing] (medium)
- 原始描述: 外部评估 #5：Managed Documents 造成 Tool Explosion，从 Unix-like tools 走向 Domain-specific OS。用户判定这是工具设计问题，算缺陷不算决策
- 复现: 当前注册工具已 30+，其中 req/defect/idea/decision/architecture/test_record/work/memory_* 等托管域工具与通用 edit/write 语义重叠；模型需在 edit 与 req(update) 之间做领域判断，工具越多误选概率越高，且每个工具签名等同公开 API
- 标签: 流程
- 优先级: P2
- 进展: 第一步做「量」不做「并」:工具面预算门禁 profiles.rs::tool_surface_budget(dev/readonly 各一条 + 记忆写路径护栏),预算取实测值不留余量。第二步按写读分离减面:2026-08-21 摘除 todowrite——它与 tracker 的 批次/进展 是同一件事的两个真源,而后者持久(过夜断了也接得上),且 dev 提示词本就写着「批次单元格是进度从外部唯一可见的地方」;实测 1019 轮里 todowrite 只用 57 次而 req+defect 用了 2378 次。dev 工具面 30 → 29,连带摘除 #todo-panel 面板/CSS/i18n/D-350 冒烟段(工具一摘没人能填它,留着就是死代码)。剩余:tracker 四件套与 research 五件套的减面按同一原则(挪进流程专用子代理快照)另行评估
- observed_head: 81a80c64d552d4da9aba0f5692c23d2b5bafb012
- observed_worktree_hash: fnv1a64:a1d1426a5522a197
- recorded_at: 1787288788389
- 停车: 排队:排在 D-568 之后恢复(原停车前提 R-353 未提交改动已不存在,按 defect-first 改排在缺陷队列末);恢复后继续 tracker 四件套与 research 五件套减面评估;恢复人:agent;解除条件:D-568

## D-746 运行画像历史输入关联审计反复全表扫描并阻塞桌面窗口 [fixing] (high)
- 复杂度: 小
- 复现: 进入运行画像触发 run_metrics_by_task；当前项目只读数据库统计 session_inputs=1664、session_events=55211、task事件=0。task_compatibility_audit 的 assigned_input_count 使用相关 EXISTS，EXPLAIN QUERY PLAN 为 SCAN input + CORRELATED SCALAR SUBQUERY + SCAN event；旧查询超过5秒诊断上限仍未完成。
- 影响: 点击运行画像时窗口事件循环长时间阻塞，历史数据越多越严重；即使尚无 task 生命周期事件也会触发。
- 来源: 2026-09-08 用户反馈：一点运行画像就会卡死
- 标签: 后端
- 根因: 每条历史输入都重新扫描全部 session_events，查询复杂度为输入数乘事件数；同步 Tauri run_metrics_by_task 在窗口命令线程执行该审计。
- 进展: 源码修复已提交：79049ee1 D-746 B1 修复运行画像历史查询阻塞。①集合查询位置 crates/kanzei-core/src/store/task.rs:28-36,397-409，用 task 事件引用的 input_id 一次性集合查询替代每条历史 input 的相关 EXISTS；测试 task.rs:881-970 覆盖重复 membership 只计一次、缺失 input 不计入、legacy input 不进入 task trend，以及 512 inputs/1024 events 的 FullscanStep ≤2048。②窗口线程解耦位置 crates/kanzei-app/src/commands/run.rs:474-489，run_metrics_by_task 改为 async 并以 spawn_blocking 承载 SQLite/历史审计；调用测试 run.rs:709-790 已改为 tokio::test。③T-1786922726990 记录当前定向回归：core store::task:: 7 passed、app commands::run::tests:: 3 passed；T-1786922726989 记录一次错误的 app --lib 命令，根因是 kanzei-app 无 library target，随后已用正确命令通过。④「安装修复版本后真实窗口点击画像可响应」尚无安装/真实窗口证据，保持 fixing；不把自动化单测替代桌面验收。
- 验收: 1.历史审计改为集合查询且重复归属、空 input_id、不存在输入、legacy 口径不变；2.大量legacy输入事件扫描步数保持线性；3.任务画像在后台阻塞线程查询且真实API测试通过；4.安装修复版本后真实窗口点击画像可响应。
- refs: R-338 R-341
- 优先级: P1
- 测试用例: 1.cargo test -p kanzei-core store::task:: --lib：7项通过，含512输入/1024旧事件扫描步数上界、重复membership/不存在input排除和legacy分类；2.cargo test -p kanzei-app commands::run::tests::：3项通过，覆盖旧rounds、分类聚合和异步task projection；3.安装修复版后选择长历史项目点击运行画像，预期页面返回且仍可切换对话和操作窗口，完成真实窗口验收后才能关闭缺陷。
- observed_head: 79049ee18550f305b42e69544dedbbec21c94972
- observed_worktree_hash: fnv1a64:43a1d1024625f1f8
- recorded_at: 1788803219458
- 阻塞: 用户：在 kzapp 安装位空闲后允许 agent 执行一次修复版本安装/启动，并用 UIA 点击长历史项目的运行画像验证窗口仍可响应；解除条件:用户

## D-748 commands 注册表只进提示词、从不展开参数,按「接线或删」原则删除 [fixing] (low)
- 复杂度: 小
- 复现: crates/kanzei-harness/src/markdown.rs:30-41 与 152-173 扫描 commands/*.md 并把「可用命令」清单拼进 core/commands_skills;全仓除测试 markdown.rs:322 外没有任何消费方,UI 与 CLI 也没有 /命令 入口,$ARGUMENTS 从不展开
- 影响: 提示词里列出实际不可用的能力(D-173 失效模式);direction_taste.md v0 已判定「接线或删,不许半吊」,用户 2026-09-25 明确不要 skills/commands 接线
- 来源: 2026-09-25 CC/Codex 对照筛选(§5.12);实施地图见 docs/design/cc_codex_alignment_impl_maps.md §14
- 标签: 核心
- 验收: ①commands 目录不再被扫描,也不出现在 system baseline;②skills 清单照旧注入;③CommandDef、HarnessDraft.commands、HarnessSnapshot::commands() 从代码中移除且编译通过;④账单 key 改为 core/skills;⑤harness_m1.md、架构索引与代码注释同步为五类注册表
- refs: D-184 docs/design/cc_codex_alignment_20260925.md docs/design/cc_codex_alignment_impl_maps.md
- 优先级: P3
- 进展: 验收核对（代码提交 1eccdaac，HEAD 现已包含）：①commands 不再扫描：MarkdownComponent 仅扫 agents/skills，scan_commands 已删除；markdown.rs 测试创建 commands/release.md 并断言 system baseline 不含命令；②skills 仍注入：markdown.rs 写入 core/skills，测试检查 build 技能及 SKILL.md 提示；③CommandDef、HarnessDraft.commands、HarnessSnapshot::commands() 与 re-export 已删除，cargo test -p kanzei-harness 171 项通过；④账单 key 已改为 core/skills；⑤harness_m1.md 与 harness.rs/lib.rs/registry.rs 注释同步五类注册，markdown.rs 注释移除 template 承诺，但架构索引行未能更新。提交仅含 defs.rs、harness.rs、lib.rs、markdown.rs、registry.rs、docs/design/harness_m1.md；未含 R-245 或 .kanzei。证据：T-1786922727024 workspace fmt、T-1786922727025 harness 171/171、T-1786922727026 harness Clippy 通过。architecture.get 仍报告五个既有非 snake_case 文档名，architecture.update 专用通道因此拒绝；不扩大到重命名无关文档，故验收⑤保留缺口、D-748 不关闭。
- observed_head: 1eccdaacf32fe1d4b2d05c9d46f17f30f64bb10a
- observed_worktree_hash: fnv1a64:cbf29ce484222325
- recorded_at: 1790341135374
- 阻塞: 
- 停车: 排队:README.md、docs/目录.md 与架构索引 harness_m1 描述已同步,architecture 工具已按 D-755 修复(均在 release/2026-09-26 发版分支);待核验后关闭;解除条件:D-755

## D-755 架构索引的既有五个非 snake_case 文档名使专用更新通道拒绝所有修改 [fixing] (medium)
- 复现: architecture.get 在 HEAD eab92725 返回 5 个 validation issue：oc-playback.md、oc-production.md、oc-idle-direction.md、oc-h3-deployment.md、oc-voice-direction.md 均被判为非 snake_case；architecture.update 因索引整体校验失败而拒绝写入，因此无法更新 D-748 要求同步的 harness_m1.md 索引描述。
- 影响: architecture 索引当前不能通过专用工具更新，任何修正单行索引元数据的需求都会被五个既有命名问题阻断。
- 来源: self-found：执行 D-748 前置核验 architecture.get 时发现；HEAD eab92725。
- 标签: 流程
- refs: D-748
- 优先级: P2
- 进展: 核验：architecture.get 当前返回 5 项命名校验错误，索引行 17、19-22 对应 docs/design/oc-playback.md、oc-production.md、oc-idle-direction.md、oc-h3-deployment.md、oc-voice-direction.md。专用校验要求文件名本身为 snake_case，且磁盘上的设计文档必须全部入索引，因此仅改索引不能解除。D-748 的现有阻塞已明确要求用户决定是否批准五份设计文档重命名并修复全仓引用，或允许 D-748 验收⑤降级；本条是该阻塞的直接原因，未改任何文档。等待用户明确选择后再继续。
- 阻塞: 
- observed_head: 4ddea60c4c646e370f621276244be9d97690c829
- observed_worktree_hash: fnv1a64:cbf29ce484222325
- recorded_at: 1790347530848
- 停车: 排队:architecture.update 已改为只拒绝本次新增的校验问题,oc-*.md 存量命名不再阻塞写入(release/2026-09-26 发版分支);复现中「HEAD eab92725」的归因应为 d4e230d8 提交的 oc-*.md;待核验后关闭;解除条件:R-364

## D-759 手机 question 卡片被 3 秒整表重绘抹掉输入,冒烟桩掉轮询造成假绿 [fixing] (high)
- 复杂度: 小
- 复现: crates/kanzei-app/mobile-pwa/app.js:206 每次轮询 container.innerHTML="" 后重建卡片,:243 setInterval(render,3000);:306 重建时输入值重置为 ask.default,多选 Set 丢失;scripts/ui-mobile-approval-smoke.mjs:143-144 以 globalThis.setInterval=()=>0 禁掉轮询后才通过;该冒烟未接入 verify.ps1/CI
- 影响: 真机上文本作答与多选最多 3 秒就被清空,D-751 验收①③在真实使用中不可用;冒烟属证据替身(假绿),D-751 关闭叙述未披露
- 来源: 波次质量审计 2026-09-26(d4e230d8..ec5dc41f,四路只读审计 + 逐条对抗核验)
- 标签: 前端
- 进展: 已在 release/2026-09-26 分支的发版提交中修复(提交号见该分支日志);待自举循环按验收逐条核验、补测试记录后关闭。
- 验收: ①轮询按 ask id 对账,已存在卡片的输入、多选状态与焦点跨轮询保留;②提交中不产生二次提交;③冒烟去掉 setInterval 桩,等待超过 3 秒后断言输入保留,并接入 ui-runtime-smoke;④取消改为显式 cancel 字段,答案文本 cancel 不再被当成取消
- refs: D-751 docs/design/bootstrap_quality_audit.md
- 优先级: P1
- 停车: 暂挂:修复已在 release/2026-09-26 发版分支完成,合入本分支前请勿另行实现(避免同一缺陷两套实现);合入后由审计会话清除本停车再核验关闭;解除条件:用户

## D-760 R-245 B8 配额:每次工具调用加锁全扫目录、超限只剩 120 字、锁超时判失败 [open] (medium)
- 复杂度: 中
- 复现: crates/kanzei-core/src/runner/tool_exec.rs:272-289 不超过阈值的结果也先取跨进程独占锁再递归扫描 tool-results(主树 shadow 已 1.4 万文件,冷扫描约 5.5 秒,同步 IO 跑在 tokio 循环里);:341-347 超限只保留 preview() 的首行 120 字;:306-319 锁超时或计量失败走 fail_tool_result_spill,已成功的输出被判 Failed 且原文丢弃;display 被整体覆盖,终端块消失
- 影响: 几乎每次工具调用增加数十毫秒到秒级阻塞且随调用次数线性恶化;配额满后模型拿不到结果正文;有副作用的命令被误报失败可能被重跑;用户「超了退回截断」的裁决被弱化且进展未披露
- 来源: 波次质量审计 2026-09-26(d4e230d8..ec5dc41f,四路只读审计 + 逐条对抗核验)
- 标签: 后端
- 进展: 已在 release/2026-09-26 分支的发版提交中修复(提交号见该分支日志);待自举循环按验收逐条核验、补测试记录后关闭。
- 验收: ①不超过外置阈值的结果不取锁、不扫描;②外置路径的计量排除 shadow 子目录;③超限时保留头 8 KiB 加尾 4 KiB 并注明省略字节与不可回取;④锁超时与计量失败降级为同样的截断且不改变工具的成败;⑤保留工具原有 display 并附配额信息,前端显示配额提示;⑥复用已存在的同 sha 外置文件不受配额阻挡
- refs: R-245 docs/design/bootstrap_quality_audit.md
- 优先级: P1
- 停车: 暂挂:修复已在 release/2026-09-26 发版分支完成,合入本分支前请勿另行实现(避免同一缺陷两套实现);合入后由审计会话清除本停车再核验关闭;解除条件:用户

## D-761 D-750 回归:依赖视图把依赖环上的条目显示为可做 [open] (medium)
- 复杂度: 小
- 复现: crates/kanzei-app/ui/12-docs-pages.js:376-381 只识别「未完成依赖:」「依赖不存在:」两种理由;引擎 crates/kanzei-tools/src/tracker/scheduling.rs:420/435-438 对环上条目只给「循环依赖:」理由,于是环成员被放进「可做(依赖已满足)」层
- 影响: 与 D-750 验收②「与引擎判定一致」相反,重新误导人工排期
- 来源: 波次质量审计 2026-09-26(d4e230d8..ec5dc41f,四路只读审计 + 逐条对抗核验)
- 标签: 前端
- 进展: 已在 release/2026-09-26 分支的发版提交中修复(提交号见该分支日志);待自举循环按验收逐条核验、补测试记录后关闭。
- 验收: ①block_reasons 含「循环依赖:」的条目进被阻塞层;②ui-runtime-smoke 增加互相依赖的环夹具并断言两条均为被阻塞
- refs: D-750 docs/design/bootstrap_quality_audit.md
- 优先级: P2
- 停车: 暂挂:修复已在 release/2026-09-26 发版分支完成,合入本分支前请勿另行实现(避免同一缺陷两套实现);合入后由审计会话清除本停车再核验关闭;解除条件:用户

## D-762 R-366 B1 偏离实施地图裁决:树根与相对路径口径错误、每次写开两次库、捕获失败记错前像 [open] (medium)
- 复杂度: 中
- 复现: crates/kanzei-tools/src/write.rs:63 tree_root 取 ctx.cwd(CLI 在子目录运行时为子目录),:65 与 edit.rs:548/792 的 rel_path 照抄 input.path;crates/kanzei-core/src/store/file_checkpoints.rs:56/111 capture 与 postimage 各开一次 SessionStore 且在 async 中同步执行;前像捕获失败后同 run 下一次触碰会把中间态记成前像;命名未用 file_checkpoint_ 前缀;StoreError::Io 文案固定为 file checkpoint
- 影响: 检查点记录口径是 B3 还原的数据基础,错误行随本次发版开始落库;每次编辑多两次 SQLite 连接并阻塞运行时;回退可能静默还原到中间态
- 来源: 波次质量审计 2026-09-26(d4e230d8..ec5dc41f,四路只读审计 + 逐条对抗核验)
- 标签: 后端
- 进展: 已在 release/2026-09-26 分支的发版提交中修复(提交号见该分支日志);待自举循环按验收逐条核验、补测试记录后关闭。
- 验收: ①tree_root 为代码树根,rel_path 由绝对路径相对树根计算;②每次写入只开一次库且在 spawn_blocking 中完成;③捕获失败留哨兵行,后续触碰不补采前像;④超过 10 MiB 的文件不整读;⑤命名改为 file_checkpoint_ 前缀,Io 文案中性,补 v24 注释
- refs: R-366 D-757 docs/design/cc_codex_alignment_impl_maps.md docs/design/bootstrap_quality_audit.md
- 优先级: P2
- 停车: 暂挂:修复已在 release/2026-09-26 发版分支完成,合入本分支前请勿另行实现(避免同一缺陷两套实现);合入后由审计会话清除本停车再核验关闭;解除条件:用户

## D-763 引擎注入规范把提交门禁写成 all-targets clippy,导致 D-754/D-758 叙述失实 [open] (low)
- 复杂度: 小
- 复现: crates/kanzei-harness/assets/default_conventions.md:75 称提交门禁跑 cargo clippy --workspace --all-targets;实际 git.rs:781-811 与 verify.ps1:134-141 都不含测试目标,只有手动触发的 ci.yml 带 --all-targets
- 影响: 模型按错误门禁描述判断风险,测试代码 lint 只在 CI 红而本地一直绿;D-754 严重度被高估,D-758 时间线叙述与事实矛盾
- 来源: 波次质量审计 2026-09-26(d4e230d8..ec5dc41f,四路只读审计 + 逐条对抗核验)
- 标签: 流程
- 进展: 已在 release/2026-09-26 分支的发版提交中修复(提交号见该分支日志);待自举循环按验收逐条核验、补测试记录后关闭。
- 验收: ①规范写明提交门禁、verify、CI 各自的 clippy 口径;②写明改测试代码时需自跑 --all-targets clippy;③守护测试保持通过
- refs: D-754 D-758 docs/design/bootstrap_quality_audit.md
- 优先级: P2
- 停车: 暂挂:修复已在 release/2026-09-26 发版分支完成,合入本分支前请勿另行实现(避免同一缺陷两套实现);合入后由审计会话清除本停车再核验关闭;解除条件:用户

## D-764 自动续跑提示被当成用户授权:R-366 写入不存在的确认记录并挂用户阻塞 [open] (medium)
- 复杂度: 中
- 复现: session_events seq 10634 循环请求用户批准 B1 冻结方案;seq 10645 唯一的 user 消息是自动续跑提示「继续推进,规则按系统提示执行。」;seq 10654 循环据此自述「仅批准该冻结方案,B2-B4 不在授权范围」,随后在 R-366 写入「确认记录: 此前用户确认仅授权 B1」并以「解除条件:用户」阻塞 B2;压缩纪要把 work next 锁定改写成了用户指令
- 影响: tracker 出现冒用用户名义的记录;用户最看重的 P1 回退被一个从未设下的授权门卡住;同样的逻辑可能放行真正需要确认的动作
- 来源: 波次质量审计 2026-09-26(d4e230d8..ec5dc41f,四路只读审计 + 逐条对抗核验)
- 标签: 核心
- 验收: ①自动续跑与 nudge 消息在模型上下文中带「自动」标记,不能被当作对 question 或方案请求的答复;②压缩纪要的用户指令清单只收用户原话;③tracker 写入声称「用户确认/授权」时须引用已作答 question 的 call_id 或真实用户消息;④规范写明:从用户已审阅的设计文档登记且带裁决的条目视为方案已确认,分批实施不需逐批授权
- refs: R-366 R-322 docs/design/bootstrap_quality_audit.md
- 优先级: P1

## D-765 自举提交只提交代码,tracker 关闭记录长期停留在未提交工作副本 [open] (medium)
- 复杂度: 中
- 复现: d4e230d8..811497b8 共 11 个自举提交无一包含 .kanzei/project;D-748~D-758 的关闭记录、R-245/R-364/R-366 进展与约 470 行测试记录只存在主树未提交副本;压缩纪要(seq 11435)显式排除 .kanzei/project/defects-archive.md
- 影响: 工作机无异地备份,工作树被重置即永久丢失关闭证据;仓库内 tracker 与提交历史不一致(HEAD 中 D-749~D-751 仍为 open)
- 来源: 波次质量审计 2026-09-26(d4e230d8..ec5dc41f,四路只读审计 + 逐条对抗核验)
- 标签: 流程
- 验收: ①结构化 git 提交把本条目对应的 tracker 块与代码同批提交(共享文件只暂存本条目的块);②关闭动作后的 tracker 改动在下一次提交前入库;③补回归测试
- refs: docs/design/bootstrap_quality_audit.md
- 优先级: P2

## D-766 发版 verify 在未提交的 .kanzei 索引上通过,已提交树的时效门禁实际是红的 [open] (medium)
- 复杂度: 小
- 复现: build-d4e230d8 提交了 docs/design/oc-*.md 与 research_library.md,但对应架构索引行留在工作副本未提交;verify.ps1 的干净判定只看 crates/scripts/.github/Cargo.*,时效门禁读的是工作副本;2026-09-26 在 ec5dc41f 干净工作树跑 verify -Full,crate_sync 的设计时效门禁失败(index 56 条 vs 磁盘 63 篇)
- 影响: 发版证据与发布提交的真实状态不一致,已提交树过不了自己的门禁
- 来源: 波次质量审计 2026-09-26(d4e230d8..ec5dc41f,四路只读审计 + 逐条对抗核验)
- 标签: 发布
- 验收: ①verify/package 对 .kanzei/project/architecture 与 docs/design 的未提交改动视为不干净,或门禁读取 HEAD 内容;②回归测试覆盖「索引行未提交」场景
- refs: docs/design/bootstrap_quality_audit.md
- 优先级: P2

## D-767 自找缺陷的验收在关单时才补写或缺失,验收对账门禁形同自证 [open] (low)
- 复杂度: 小
- 复现: defects-archive 中 D-752、D-754、D-756 的「验收」字段出现在 observed_head/recorded_at 之后,是关单那次 update 才追加;D-757、D-758 无验收字段仍被关成 fixed;action_helpers.rs 的对账门禁找不到带圈条款即放行
- 影响: 三方核对(验收原文、实现、关闭叙述)被实现者现写的条款取代,沉默降级无法被机制发现
- 来源: 波次质量审计 2026-09-26(d4e230d8..ec5dc41f,四路只读审计 + 逐条对抗核验)
- 标签: 流程
- 验收: ①self-found 缺陷登记时必须带验收;②关闭时若验收在实施之后才首次写入,关闭遥测标记 acceptance_post_hoc;③无验收的关闭在遥测中记为不适用而非已对账
- refs: docs/design/bootstrap_quality_audit.md
- 优先级: P3

## D-768 D-756 只改了 schema 提示,update/close 漏传顶层 id 的运行期报错仍不指路 [open] (low)
- 复杂度: 小
- 复现: crates/kanzei-tools/src/tracker/actions.rs:372-374 仍只报「`id` is required」,不检测 fields 里的 id;Anthropic 线路会摘掉顶层 allOf(anthropic.rs:416-433),DeepSeek/Chat 非 strict 不强制;schema 条件只覆盖 update 不含 close
- 影响: 漏传顶层 id 的问题在各 provider 上都只得到一句描述提示,出错后的纠正信息没有改善
- 来源: 波次质量审计 2026-09-26(d4e230d8..ec5dc41f,四路只读审计 + 逐条对抗核验)
- 标签: 后端
- 验收: ①id 缺失且 fields 含 id 或编号时返回 MISSING_TOP_LEVEL_ID 定向纠错;②schema 条件覆盖 update 与 close;③单测走 update_close 真实分支
- refs: D-756 docs/design/bootstrap_quality_audit.md
- 优先级: P3

## D-769 ci.yml 只有手动触发,代码注释与规范却称 CI 每次 push 兜底 [open] (low)
- 复杂度: 小
- 复现: .github/workflows/ci.yml 只有 workflow_dispatch;crates/kanzei-tools/src/git.rs:791-792 注释称「CI 每次 push 兜住」;d4e230d8 发版时 CI 口径的 all-targets clippy 已是红的而无人发现
- 影响: 测试代码 lint 与全量测试没有任何自动门禁,注释给出错误安全感
- 来源: 波次质量审计 2026-09-26(d4e230d8..ec5dc41f,四路只读审计 + 逐条对抗核验)
- 标签: 发布
- 验收: ①二选一:给 ci.yml 加 push 触发,或改正注释并让 package/full verify 跑一次 all-targets clippy;②规范与注释口径一致
- refs: docs/design/bootstrap_quality_audit.md
- 优先级: P3
