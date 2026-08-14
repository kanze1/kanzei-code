# Requirements

## R-186 跨树越界检测与回滚:ManagedSnapshot 范围从托管文档扩到「不属于本线的 worktree」 [doing]
- 优先级: P0
- 复杂度: 中
- 标签: 核心
- 归属: kanzei
- 阶段: 3
- 证据等级: E1(D-174 已交付同哲学的实现;本条是范围扩展)
- refs: D-267(本条是它的替代交付) D-173 D-174 R-183 R-184 R-177 D-258
- 来源: 2026-08-11 用户定调砍掉 bash 权限中间档后的替代方案。原话:「这些直接砍了,没啥用说真的,并行自举本来就是激进的玩法」。
- 为什么是这个形态(本条的全部理由): 并行下真正要防的**不是恶意,是串台**——A 线的命令跑进 B 线的树、把人家**未提交**的活覆盖了。而**命令语法闸门恰恰防不住这个**:`cd ../other && rm -rf` 里没有一个可疑 token,`cargo` 也是合法程序、其 `build.rs` 能干任何事。
  正解沿用本仓既有哲学。设计基线明写着 bash 对托管文档的保护「**在结果侧**(执行前后快照比对 + 隔离留证 + 整体回滚),**不在权限层**」——D-173/D-174 已经把这条路走通并有实现(`crates/kanzei-tools/src/bash.rs` 的 `ManagedSnapshot::capture` / `is_complete`)。它的关键优势是**不关心命令长什么样**,所以 `cd ../other` 与 `cargo run` 里 build.rs 干的坏事**一视同仁抓得到**——这正是闸门做不到的那一半。
- 内容: ①把 `ManagedSnapshot` 的保护范围从「主根 `.kanzei` 托管文档」扩到「**不属于本线的 worktree**」:执行前拍快照的集合 = 托管文档 ∪ 其它线的工作树;②越界写入的处置沿用 D-174 既有形态(隔离留证 + 整体回滚 + 归因到 owner run),**不新造机制**;③「本线的树」由 `ProcessHandle.worktree_path` 给出(前置 R-177),主树进程的"本线"= 主根;④性能:快照集合可能很大,按**只对其它线的树做 mtime 级粗筛、命中再细查**收敛,不得让每条 bash 都全量哈希(D-233 的教训:同步全量读+哈希会把主线程占死);⑤越界事件进轨迹,**同时作为 R-184 冲突带的数据源**——"谁写了不属于自己的文件"与"谁和谁改了同一个文件"是同一份数据,不要采两次。
- 边界: **不做事前拦截**(那是被砍掉的 D-267 的路子)。不保护未纳入任何线的目录(用户自己的其它项目不在范围内——本条只管本仓的树之间)。不做跨机器。`ManagedSnapshot` 对**托管文档**的既有行为**一个字不改**(它是 D-173/D-174 的交付,只加范围不改语义)。
- 验收: ①A 线执行 `cd <B线树> && <写操作>` 后:改动被检测、被隔离留证、被回滚,B 线的工作树**逐字节复原**,有实测轨迹(不是只断言函数返回);②归因正确:轨迹里指出是哪条线(owner run)越的界;③**`cargo run` 里 build.rs 写别人的树**同样被抓——这条是本条相对闸门的核心优势,必须有定向测试;④托管文档的既有保护行为无回归(D-174 既有测试全绿);⑤性能:单条 bash 的快照开销有实测数字,N 条线时不随 N 线性劣化到不可用(给出实测,不接受"看起来还行");⑥越界事件与 R-184 冲突带共用同一份数据,不存在两套采集(机械核验:grep 只有一处采集点)。
- 依赖: 
- 前置(不写进依赖,按 D-239 教训): **R-177**(要有 `worktree_path` 才知道"本线的树"是哪棵)。R-177 之前可以先做托管文档侧的重构与 mtime 粗筛。
- 取活依据: engine:无可执行 WIP，按 requirement-first 选择队首 R-186
- 进展: 自动运行已认领(doing)。2026-08-13 用户明确指示暂停本条、先交付 R-200(测试隔离夹具)并按其批次发版——本条 park,不占可执行槽位。未开工。
- 阻塞: 2026-08-14 复核:原阻塞的解除条件「R-200 及其后续完成后再开」已达成——R-200 已 done 并归档。剩的只是队列位置:本条 P0 但 R-202 占着唯一 WIP 槽,且缺陷队列现有 D-357/D-358/D-359 三条未阻塞条目按 defect-first 排在前面。用户原话是「先做 R-200 再发版,不是做这条」,发版已在本轮执行。解除动作: 用户说一声恢复推进,或 R-202 与缺陷队列清空后按队列自然取到本条。解除人: 用户(一句话即可,不需要额外信息)。
- observed_head: d124749aabe65ec0cde4f2280c9583dd4f33be40
- observed_worktree_hash: fnv1a64:794cece9eb0bfcad
- recorded_at: 1786609593506

## R-183 kz 无人值守执行通道:非交互直接放行 bash + 可审计轨迹(原「预授权集」随 D-267 作废) [doing]
- **2026-08-11 改写(用户定调,随 D-267 关闭为 dropped)**: 原标题里的「permission 规则 worktree 继承主根、可审计预授权集」两项**作废**——它们服务的是 D-267 的中间档,而中间档已被砍掉(理由见 D-267 关闭说明:挡不住有意的、被绕过两次、威胁模型里没有「模型是敌人」)。**本条大幅缩小**:非交互模式下 bash 直接放行,防线整体挪到结果侧(R-186)。
  下方原「内容」「验收」保留作为历史,**实施以本节为准**。
- 优先级: P0
- 复杂度: 中
- 标签: 核心
- 归属: kanzei
- 阶段: 3
- 证据等级: E1(2026-08-11 实测三次全失败 + 读码定位)
- refs: R-182 R-177 R-030 docs/design/parallel_read_serial_write_orchestration.md
- 来源: 2026-08-11 搭任务级并行实测时,**`kz run` 在 worktree 里无法无人值守跑**,是当天唯一让实验彻底停摆的硬卡点(另一个候选载体 `claude -p` 因 OAuth token 被吊销同样不可用)。任务级并行的前提是「N 条线各自跑到底」,没有这个通道就只能靠外部 CLI。
- 现状与缺口(逐点读码核实): 
  ①**EOF 落 Deny**:`crates/kanzei/src/main.rs:394-416` 的权限分支读 `std::io::stdin().read_line()`,后台运行时 stdin 是 EOF → 空行 → 落 `_ => AskReply::Deny`。不挂死,但**每一次写和每一条 bash 都被拒**,agent 寸步难行。
  ②**permission 规则的 workdir 钉死主根**:`.kanzei/kanzei.toml` 的 24 条 `[[permissions.rules]]` 里,后半段规则的 resource 是 JSON,内含 `"workdir":"c:/users/kanzei/documents/kanzei code"`。从 worktree 跑时 workdir 是 worktree 路径,**这些规则一条都匹配不上**——线启动时等于空白允许清单。
  ③**`cargo` 根本不在允许清单**:全部 24 条规则里没有任何 `cargo *`,而 Rust 任务的验证全靠它。
- 内容: ①非交互检测 + 显式策略:无 TTY 时不再落 Deny,改为按配置的**非交互默认策略**(建议三态:`deny`(现状,保守) / `rules-only`(只认预授权规则,规则外拒) / `allow-listed`(规则 + 本次运行的显式 allowlist));策略必须显式配置,**不提供"全放行"的隐式默认**。②permission 规则的 worktree 继承:worktree 内运行时,规则匹配按**主根**而非 cwd 解析 workdir(与 R-182 的主根重定向同一条原则),避免线一启动就没有任何授权。③可审计:非交互模式下每一次自动放行都落轨迹(动作、资源、命中的规则、时刻),`kz` 退出时给出汇总;拒绝同样可见(D-004 口径)。④补齐开发所需的基础规则模板(cargo/node/git 的只读与构建子集),放进新建配置的注释模板(与 R-172 同族)。
- 边界: 不做「全部自动同意」的开关——那等于把权限系统关掉,与仓库既有的硬 deny 纪律冲突。不改 profile/agent 体系。不做桌面端的无人值守(桌面端有 UI 可问,不是同一个问题)。
- 验收: ①`kz run` 在 worktree 里后台运行(stdin 关闭)能完成一次真实的「改代码 → `cargo test` → 提交」闭环,不因权限被拒而中断;②非交互默认策略三态各有测试,**缺省仍是 `deny`**(不改变现有用户的行为,旧配置无该键时行为不变);③从 worktree 运行时,主根的 permission 规则能命中(有测试直接断言同一条规则在主根与 worktree 下匹配结果一致);④每次自动放行有可查轨迹,含命中的规则原文;⑤无 TTY 检测本身有测试(不是靠"读到 EOF"倒推)。
- 依赖: 
- 取活依据: engine:无可执行 WIP，按 defect-first 选择队首 R-183
- 进展: 批2 完成(2026-08-16,提交 ba0726f)。轨迹含命中规则原文(验收④):①Ruleset::evaluate_with_rule(与 evaluate 同一判定,额外返回 last-match-wins 命中的普通规则;硬 deny/无匹配 None,D-051 降级仍如实返回规则)+ HarnessSnapshot::evaluate_with_rule 委托;②RunEvent::PermissionResolved 加 rule: Option<String>;③drive.rs 两处 ruleset 评估站点(串行门禁/并行 wave deny)改用 evaluate_with_rule 填规则原文,describe_rule 展示格式 `action resource => effect`;会话层决策 rule=None;④CLI 打印 deny/会话层决策时附 [规则: ...](run.rs 消费点补 .. 解构)。测试:permission.rs 补 evaluate_with_rule 三场景(命中返回/硬 deny None/无匹配 Ask-None)。验证:permission 30 + core 193 + kanzei 26 + app 160 全过(T-1786731021);fmt/clippy 绿;push 已到 origin/dev。批3(收口):验收③ worktree 主根规则命中测试(BashTool workdir 来源确认——规则匹配是否按主根,补测试断言同规则主根/worktree 一致)+ 验收① 非交互闭环集成验证 + 全量。
- 阻塞: 
- observed_head: ba0726ff938021de1abf68831a1b5daabc26d0ba
- observed_worktree_hash: fnv1a64:cbf29ce484222325
- recorded_at: 1786731061013
- 进展: [reopen 2026-08-14] D-359 修复后用正路退回:原阻塞(让位 D-332)的解除条件早已达成(D-332 已 fixed 归档);本条 doing 是 engine 自动认领留下的空档,进展字段自述从未开工、无进展锚点。退回 todo 按 P0 重新入队,不再靠往阻塞字段塞理由把它挪出 WIP 槽。
- 批次: 2/3

## R-221 research 模式重定位:按 docs/design/research_mode.md 分批实施独立深度研究模式(文献+仓库调研,论文级产出) [doing]
- 优先级: P2
- 复杂度: 大
- 标签: 后端 前端 harness
- 来源: 2026-08-12 八维度审计维度8;设计文档 docs/design/research_mode.md(§2 八个定调点待用户逐项确认后动工)。
- 背景: research 模式骨架完整但形态错位(面向网络调研)且零使用(state.db 266 条 episodes 零调用 websearch/source/finding,.kanzei/research 全 git 历史只有空模板);真实勘察全在 dev 完成且结论无固定落点(勘察报告被 D-294 单行不变式折成单行塞进度字段);证据等级 E0-E4 被双重语义挪用;research/memory.md 是绕开记忆控制平面的第二套无校验记忆。
- 内容: 原按设计文档六批实施,2026-08-14 用户定调后重排:①档位收口(桌面注册 ReadonlyProfile、files/git 只读入列)——**bash 硬 deny 一项作废**,新定位要跑 LaTeX 编译与绘图,须改为白名单或专用工具通道;②topic 工件落点——**待重推**,论文形态需容纳 paper.tex/figures/refs.bib,不是单个 report.md;③证据等级 V 表进 conventions——**已定**,四档待按文献口径扩展;④回流通道(finding→req/defect 草稿)——**待重推**,research 独立后是否仍属本模式职责需重判;⑤记忆一元化——**待复核**,论文引用管理(refs.bib)与记忆晋升是两件事;⑥三形态收敛——**作废**,research 独立后不与 dev 侧 task 勘察/SCOUT_ROLES 收敛。
- 边界: research 不可提交 git、不动既有条目状态(add 草稿除外);不做报告 schema 校验。「不可写 docs/design」一条待重推(新定位下产出是论文而非设计文档,问法需重新表述)。**dev 侧「先计划后自举」的勘察工件落点问题不由本条承接**——那是独立课题,需另立条目。
- 验收: 以设计文档 §7 总则为准——一条真实 R- 条目的 勘察→报告→登记→dev 实施 完整链路有轨迹;每批验收见设计文档 §6。
- refs: D-276 R-201 D-304
- 取活依据: engine:无可执行 WIP，按 defect-first 选择队首 R-221
- 进展: 2026-08-16 取活。勘察结论:R-221 的设计真源 docs/design/research_mode.md 状态为「设计基线草案(2026-08-12 八维度审计维度8 产出;定调点待用户逐项确认后转正)」——§2 的八个定调点(主形态/工件落点/证据等级 V 表/回流通道/记忆一元化/档位矩阵/可写 docs 边界/三形态收敛)全部标注「待用户确认」,括号内为本设计的默认建议。按 §1「需求边界不清楚时必须先提问确认,不允许在关键问题上自行假设后直接实现」,八个定调点未获用户拍板前实施会踩边界(如「research 不可写 docs/design」「证据等级单列 V 表」都是用户层面决策)。现状盘点(供解除阻塞后立即开工):批1 档位收口的 files/git 只读已在 R-218 完成(SubagentBase 6 件套),ReadonlyProfile 与 bash 硬 deny+替代指引是既有模式(profiles.rs:652-658 先例);批2-批6 的 topic 工件/证据口径/回流/记忆/三形态均未动。阻塞:等用户对 research_mode.md §2 八个定调点逐项拍板(解除人:用户;方案与默认建议已在 docs/design/research_mode.md §2)。 || 2026-08-14 用户过审:定调点1 被否——research 是**独立深度研究模式**(文献+仓库深度调研,产出论文/LaTeX/图表),不是「先计划后自举」载体,网络检索是主力不是辅助;连带定调点8(三形态收敛)作废;定调点3(V 表单列)已定,四档待按文献口径扩展;定调点2/4/6/7 待 §1 重写后重推,其中6 已知冲突(原案 bash 硬 deny 与跑 LaTeX 编译/绘图直接矛盾)。设计文档 §1/§2 已同步改写,§3 之后各节按旧命题写成尚未同步。
- 阻塞: 两条并存——①**排期**:用户定调实施在 dev 稳定之后,当前不开工不占可执行槽位;②**设计**:research_mode.md §3 以后各节仍按已作废的旧命题(代码库勘察载体)写成,须先随 §1 新定位重写并把 §2 待重推的四条(工件落点/回流通道/档位矩阵/可写 docs)重新过审,才具备实施条件。解除人:用户(两条都是)
- observed_head: b644f1657f2aadede85b26ef65050605740ceb04
- observed_worktree_hash: fnv1a64:794cece9eb0bfcad
- recorded_at: 1786633950047

## R-216 记忆写入侧质量三闸:近似去重下沉 store.add 双 scope、[fp:] 指纹一致性校验、tracker 交付状态内容拒收 [doing]
- 优先级: P2
- 复杂度: 中
- 标签: 后端 记忆
- 来源: 2026-08-12 八维度审计(§5)。M-055/M-056 于近似去重上线当天英文复述 M-044 并携带编造指纹——「假指纹立即污染注入」经反证驳回(FingerprintIndex 只收 active 且不扫标题),但穿透与伪造本身实证成立;另有 6 条交付状态类内容落进记忆与 tracker 重复。
- 内容: ①classify_novelty 的 FTS 语义探测下沉进 store.add 作为硬闸(Uncertain 即拒并返回候选),查重范围扩到双 scope;②新条目携带的 [fp:] 必须与来源 note 中引擎生成的指纹逐字一致,拒绝自造;③标题/subject 命中「R-/D- 编号+已交付/勿重复/验收边界」形态时拒绝并指路 tracker(或强制挂 refs 并随条目关闭自动 deprecate)。
- 验收: ①复刻「英文改写 M-044」场景被拦并指路 memory_update(单测);②伪造指纹的 add 被拒;③存量 6 条交付状态记忆逐条处置;④各拦截路径有单测。
- refs: R-194 R-195 R-196 D-299 D-282
- 取活依据: engine:无可执行 WIP，按 defect-first 选择队首 R-216
- 进展: 2026-08-16 收口此前引擎取活留下的半成品实现(工作树未提交,6 测试红)。已完成:①三闸实现确认完整(store.rs add 内:交付状态拒收 has_tracker_id、指纹一致性 fp_markers、语义探测下沉 classify_novelty 双 scope);②修复 6 个失败 fixture(5 个自造指纹被新指纹闸拦——merge_gate/find_by_marker/merge_conservative/merge_自动搬运 注入来源 note 或 force,1 个 novelty_gate 语义断言适配 R-216 口径);③新增 3 个验收单测:自造指纹的add被拒_来源note指纹放行、交付状态内容被拒并指路tracker、英文改写被add硬闸拦截返回候选。验证:memory 95 passed + kanzei-tools 346 passed + clippy/fmt 全过。验收对照:①英文改写被拦并指路 memory_update——英文改写被add硬闸拦截返回候选 测试(Uncertain 返回候选);②伪造指纹的 add 被拒——自造指纹的add被拒 测试;③存量 6 条交付状态记忆逐条处置——数据工作待做(见剩余);④各拦截路径有单测——三测试覆盖指纹/交付状态/语义三闸。剩余:验收③存量 6 条交付状态记忆逐条处置(数据工作)。
- 阻塞: 2026-08-14 复核:本条不存在外部阻塞——原阻塞写的「解除人:本 agent 后续轮」是自己阻塞自己,那不是阻塞,是没做完。真实状态:三闸实现已完整交付且全绿(memory 95 + kanzei-tools 346 passed),验收①②④齐,只剩验收③的数据工作(逐条查 memory 库定位 6 条交付状态记忆并归档/改写)。当前挂 doing 却被阻塞字段整条 park,是因为 R-202 占着唯一 WIP 槽——清掉阻塞会让 work next 判 wip_violation 禁止全线取活(本轮在 R-183 上实测过)。用户 2026-08-14 明确本条数据工作不交给本会话代做。解除动作: R-202 关闭腾出槽位后直接续做验收③并关闭本条,不需要用户拍板。解除人: agent。
- observed_head: a104ba12af981e0e591aff0c9a5057385ce2f854
- observed_worktree_hash: fnv1a64:025c9fc9adc6d9d2
- recorded_at: 1786637389551

## R-195 candidate 记忆的晋升与清退闭环:存量 5 条无人验收,最后一次晋升停在 2026-08-10 [doing]
- 内容: 给 candidate 定一条会被执行的闸门,形态不在本条强行拍板:或按复发计数自动晋升(计数已可用),或轮末/每 N 轮让 manager 逐条判定晋升与清退,超期未处置的自动 deprecate 归档。同时把存量 5 条走一遍该流程。
- 复杂度: 中
- 来源: 2026-08-12 记忆库存清理:22 条 candidate 里 15 条是重复或空正文条目(已置 deprecated 归档,见提交 2bc5899/216120e),剩 5 条 M-034/M-035/M-037/M-038/M-040 自 2026-08-10 起无人处置;最后一次成功晋升是 M-032(2026-08-10 21:31)。
- 标签: 核心
- 现状: ①promote 有 provenance 硬约束(store.rs:promote 要求至少一条 episode 证据),但没有任何常规动作会产出这种证据,于是没人晋升得了;②三段晋升(第 2 次建 candidate、第 3 次+带修复证据晋升)依赖跨轮复发计数,而计数此前因指纹里含命令载荷永远停在 1——该病灶已在 f104890 修掉(mask_volatile_payload + normalize_fp_marker),计数从此能涨;③即便计数能涨,晋升仍需一个会真正被执行的判定动作。
- 边界: 不改「未验证不注入」的取舍(R-165):本条不是要让 candidate 参与召回,而是不让它永远躺着。已在 f104890 落地的部分(candidate 对去重与复发检测可见)不重做。
- 验收: ①存量 5 条 candidate 全部有归宿(晋升 active 或 deprecated 归档),逐条给出依据;②有机制测试:满足条件的 candidate 能被自动处置,不满足的不动;③candidate 存量不再单调增长——用 index.db 与文件数给出前后对照。
- 优先级: P2
- 批次: 2/2
- 进展: 用户于本轮明确选择暂存本条，暂不推进；保留原验收与 0/2 计划，待 R-236 释放 WIP 槽后再恢复。来源：用户本轮选择 1。
- observed_head: 79d3c4e383a13032ff26c4cd0a13bcd74128c2f2
- observed_worktree_hash: fnv1a64:fe871977f10a5179
- recorded_at: 1786648602102
- 取活依据: engine:唯一可执行 WIP 是 R-195，必须先恢复它
- 用户挂起: 是；用户明确选择暂存 R-195，待 R-236 完成后恢复。
- 阻塞: 2026-08-14 复核:原阻塞「待 R-236 完成后决定恢复本条」的前置已达成——R-236(上下文压缩重设计)已 done 并归档。所以这条现在是纯决策点,不是等待:candidate 记忆的晋升与清退闭环要不要现在恢复推进。相关:R-235(28 条零证据 active 记忆逐条拍板)是同一批记忆治理工作,两条宜一起定。解除动作: 用户说恢复即按 P2 入队。解除人: 用户。

## R-235 存量 28 条零证据 active 记忆逐条复核:保留(存量豁免)或降级 candidate,用户拍板 [todo]
- 优先级: P3
- 内容: 对 28 条零证据 active 记忆逐条复核:保留(存量豁免,接受不可计量)或降级 candidate(严格符合无来源不入 active,代价是不可检索注入)。复核结果与依据落到 memory 系统设计文档或本条目关闭证据。
- 复杂度: 小
- 来源: R-213 关闭时盘点发现(R-213 验收③处置的承接)
- 标签: 后端
- 背景: R-213 盘点:state.db 311 条 episode、memory_sources 0 行,project 域 28 条 active 记忆(M-001~M-063)全部零证据(global 域无条目)。这些是 provenance 门禁上线前由用户/交互会话/manager 产生的既有资产,source 字段均无机器可链接的 run_id,历史回填=变相伪造,不可行。R-213 的处置定为存量豁免+文档化,但控制平面「用数据判断记忆是否改善决策」对这些条目无法计量,保留还是逐条降级应由用户拍板。
- 验收: ①28 条清单逐条给出保留/降级结论与依据;②结论落地(设计文档或关闭证据);③如选择降级,操作后搜索不再命中 candidate 条目。
- 阻塞: 用户: 28 条零证据 active 记忆保留(存量豁免)或降级 candidate 需用户逐条拍板,解除权不在 agent。解除动作: 用户给出拍板结论(全部保留 / 逐条降级清单)后按结论落地并关闭。解除人: 用户。

## R-193 plan勾选响应延迟优化需求 [doing]
- 复杂度: 中
- 标签: 前端
- 验收: plan勾选项点击后实现即时视觉反馈和状态更新
- 优先级: P2
- 取活依据: engine:无可执行 WIP，按 defect-first 选择队首 R-193
- 批次: 0/1
- 进展: 2026-08-16 认领并勘察。现状:todo/当前计划面板由 tool-end 事件的 display.kind==='todo' 渲染(07-events.js:223-224 renderTodoPanel),条目纯展示状态文本(○/●/✓/×,07-events.js:52-63),无任何点击/勾选交互绑定。R-193 验收『plan勾选项点击后即时视觉反馈和状态更新』隐含存在可勾选的 plan 列表,但当前实现无勾选交互、无内容/来源字段、未指明勾选动作应写入哪个真源(前端本地态?后端 todo 状态?)。按 §1.1 需求边界不清不得自行假设实现;需用户澄清:①plan 指当前计划(todo)面板还是其它列表;②勾选后状态写哪里(仅前端视觉 / 调用某命令持久化);③当前是否有已感知的『响应延迟』具体场景(点击无反馈?状态更新慢?)。本条保持 doing,待澄清后实现。批次:0/1。
- observed_head: 273c4a3cc6138331a4c07469127773835af001ef
- observed_worktree_hash: fnv1a64:794cece9eb0bfcad
- recorded_at: 1786656932641
- 阻塞: 用户: R-193 缺内容/来源/交互定义,验收仅一句『plan勾选项点击后即时视觉反馈和状态更新』;需用户澄清:①plan 指哪个面板(当前计划 todo 面板还是其它);②勾选动作状态写哪里(前端视觉 / 后端命令持久化);③当前『响应延迟』的具体场景。解除动作:用户给出澄清后实现。解除人: 用户。

## R-174 子代理面板与并发度口径:独立 Running/Finished 面板、单条停止与完整 transcript [doing]
- 优先级: P0
- 复杂度: 中
- 标签: 前端
- 归属: kanzei
- 阶段: 3
- 证据等级: E1(现状逐点读码核实,行号为 2026-08-10 dev HEAD)
- 来源: 2026-08-10 用户看过 Claude Code 的后台子代理面板后定调:kanzei 的子代理要走这个执行模型,而且比它更激进。四个轴——①后台化(跨轮存活、主代理派完不阻塞、完成发通知)②子代理能写(打破只读白名单、自持写租约)③并发度放开(远不止现在的 8)④可对话(给正在跑的子代理发消息带原上下文续跑)——**都要,但必须分级实现**(用户原话:「都要,但是你说的这些点得分级实现,确实改动大,风险多」)。参照物形态(Claude Code 面板实测):独立 Background tasks 面板,分 Running / Finished N 两区;每条显示名称、类型、已运行时长、累计 token、工具调用次数、当前正在用的工具名、View transcript 链接、单条停止按钮;面板有 Clear。本条是四轴里最便宜、可独立交付的一段(可观察 + 并发度口径),不依赖后台化。
- 设计定位: 四轴分级第 1 级——先把子代理变成「看得见、停得住、查得到」的对象;后台化(R-175)与写权(R-176)在此之上叠加
- 既有能力(§1.25 显式标注,不得重复申报为本次产出): 并发度**已经是可配项**——`max_tasks_per_turn` 在 crates/kanzei-harness/src/config.rs:59 是 `Option<usize>` 字段,:90-92 `unwrap_or(8).max(1)` 给默认 8 且**无上限钳制**;设置页已有「单轮子代理数上限」输入框(crates/kanzei-app/ui/index.html:469-470 `set-max-tasks`、ui/16-settings.js:187 与 :420、crates/kanzei-app/src/settings.rs:287-288/497/519/531);往返单测已钉死「没填的键不写进文件 / 没填走内置默认 8」(settings.rs:749-752)、serde default 单测已存在(config.rs:918-920、:936-939 越界回落 1)。因此「从固定 8 改为可配」这件事**无需再做**。
- 关键现状(本组三条需求的共同前置): 桌面端主对话**根本不注册 task 工具**——crates/kanzei-core/src/runner/drive.rs:57 只在 `subagent.is_some() && !config.execution_policy.is_serial_writer()` 时 push `task_spec()`,而 crates/kanzei-app/src/run.rs:107-108 给主对话**无条件**设 `ExecutionPolicy::ReadParallelWriteSerial`(`is_serial_writer()==true`,见 crates/kanzei-harness/src/orchestration.rs:21-23)。所以并行子代理在桌面端当前是**全禁**状态:drive.rs:410-503 的轮内并发批与 crates/kanzei-core/src/runner/subagent.rs:163-176 的读槽登记代码不可达,`max_tasks_per_turn` 配了也没有生效路径。该回归由 **R-173**(阶段编排对象)的阶段感知策略修复,是本条与 R-175/R-176 的共同前置——本条的面板与并发度实测必须在 R-173 修复后才能在桌面端取证。
- 内容: ①并发度口径收口(**不是重做配置**):复核默认 8 是否上调(用户要「远不止 8」),并在设置页把该值与「桌面端当前不生效」的事实对用户说清;溢出分支文案沿用 drive.rs:441-444 既有实现。②新增**独立「子代理」面板**——不再只作为活动面板(#bg-panel,ui/index.html:630-650)里 `bg-type-filter=agent` 的一个筛选项:Running / Finished N 分区,每条显示 名称 / 类型 / 已运行时长 / 累计 token / 工具调用次数 / **当前正在用的工具名** / 单条停止 / 打开 transcript,面板有 Clear。③单条停止通道:现状 ui/06-activity.js:261 注释明写「子代理没有单条停止通道,只能停整轮」,本条要消灭这个缺口。④可查看单个子代理的**完整 transcript**(工具调用序列 + 每次调用的入参与输出),不再只有 R-095 的摘要维度(内部调用数 / 当前步骤 / 成败 / 耗时)。
- 边界: 不做后台化(R-175)、不做写权(R-176);面板本条只需渲染**轮内并发**的子代理,跨轮存活条目待 R-175 提供数据后再接。不改 `max_tasks_per_turn` 的配置通道本身(已可用),只调默认值口径与设置页说明。
- 验收: ①并发度实测:`kanzei.toml [limits] max_tasks_per_turn = N`(N 取远大于 8 的值)后,同轮派发 N 个 task 全部执行、第 N+1 个才落 drive.rs:441-444 的溢出错误,有轨迹或日志证据;②旧配置无该键时行为不变——config.rs 既有 serde default 单测保持绿(若本条上调默认值,须同步更新 :918-920 断言并保留「缺键=内置默认」语义),settings.rs:745-752 往返单测保持绿(保存不丢字段);③面板存在且分区正确,每条的 名称/类型/时长/token/工具调用数/当前工具名 六个字段**均取自真实 RunEvent**(ToolStart/TaskProgress/ToolEnd),冒烟脚本用桩事件逐字段断言渲染出真实值而非常量占位;④单条停止真能停:点击后该子代理不再产出 TaskProgress、以「被停」终态收尾、读槽被释放,有实测证据(仅改 UI 类名/状态不算通过);⑤transcript 有真实数据源:能查看单个子代理的完整工具调用序列与每次调用的入参/输出——§1.25 明令「只展示但未接入真实数据源的界面壳不算完成」,不得以摘要冒充 transcript;⑥前端改动有冒烟断言:`node --check` + `node scripts/ui-runtime-smoke.mjs`,分区切换、单条停止、打开 transcript 三个新交互各有对应断言(§1.3);⑦桌面端可达性:R-173 修复前置回归后,在桌面端主对话实测面板真出现子代理条目(不能只在 CLI 或单测里成立)。
- refs: R-095 R-117 R-173 R-175 R-176
- 依赖: 
- 进展: 批1-3已提交(9179ae8/68ee84ec/25ea2c0),cargo test --workspace 全量全绿。验收①并发度实测✓(集成测试+轨迹)、②旧配置无键行为不变✓(serde default 测试绿)、③面板分区与六字段真实数据✓(冒烟逐字段断言)、④单条停止✓(stop_task + task_cancel_parallel.rs 实测)、⑤transcript 真实数据源✓(TaskTrace.input 渲染,冒烟断言入参)、⑥冒烟断言✓(分区切换/停止/transcript/被停终态/Clear 均有断言)。仅剩验收⑦「桌面端主对话实测面板真出现子代理条目」未闭环——需要构建新版 kzapp 安装,2026-08-11 用户定调:先不装,等下次发版一起实测。本条保持 doing 待发版,不占可执行槽位。
  2026-08-11 本次发布补齐面板生命周期：停止→已完成→已关闭→删除；关闭/删除只改变当前 UI 条目，后端 transcript/审计保留，真实 stop_task 仍是停止通道。主代理权限边界在系统提示与子代理 task_spec 中显式固化：子代理仅 read/glob/grep，写入、比对、合并和发版由主代理负责。发版后验收⑦转为用户桌面实测。
  ①**前置回归已解除**——「桌面端主对话根本不注册 task 工具」那条(本条与 R-175/R-176 共同记录的前置)已由 R-173 批4.5 修掉(`e933262`),验收⑦现在可以真去桌面端取证了。
  ②**验收③已部分交付**——R-173 收尾时把编排派发的勘察/复核子代理接上了活动面板(`ff287c4`):按 `input.phase` 分「勘察/复核」两组、显示角色名与**当前工具名**(取 `kz:task-progress` 的 `trace.name`)、运行时长、内部调用数,超时与失败分开成两种终态,冒烟有 6 组反证锁死。**它刻意复用 `#bg-list` 没有新建平行面板**——本条要做的独立面板应当在此之上演进,不是另起炉灶。仍缺:累计 token、Clear、Running/Finished 两区(现在是按阶段分组,不是按运行状态)。
  ③**验收④单条停止的最小改法已备**:目前 `dispatch_roles` 的 future 集合由屏障统一驱动,没有对外暴露的 per-role cancel handle。改法 = 每角色配一个 `CancellationToken` + 新 Tauri 命令按 role 触发,取消后该角色以 `ScoutOutcome::Failed("cancelled")` 进终态——屏障照常收敛,不会挂住。
  ④**两条形态决策留给本条拍**:(a) 编排派发的 8 条同时也会在**主对话**里各生成一个工具块(`chatToolStart` 无条件调用),信息没丢但每个自主推进轮多 8 个块,可能偏吵;(b) 前端条目的 `id` 就是角色名,而角色跨轮复用,所以当前实现是**每角色只留最新一轮**(跨轮定格的 bug 已修成"原地复位")。要保住历史轮次得让后端给 `role@round` 之类的唯一键。
  另:验收①②的「并发度可配」部分是**既有能力**(见本条「既有能力」字段),不要重做。

- 批次: 3/5

- 阻塞: 2026-08-14 前置已满足:新版 build-9a06e05 已发布(dist/kanzei-setup-9a06e05.exe),原阻塞「先不装,等下次发版一起实测」的等待对象到位。剩验收⑦一条实测动作:装新版后在桌面端主对话发一个会派子代理的任务,看独立 Running/Finished 面板是否真出现子代理条目(以及单条停止与完整 transcript 是否可用)。解除动作: 用户实测并确认后关闭本条。解除人: 用户。

## R-101 桌面端/前端 E2 测试 harness 与延期 E2 清单 [doing]
- 复杂度: 大
- 优先级: P0
- 归属: kanzei
- 背景: 多条缺陷按 conventions §1.2「可用即关闭」关闭,其验证增强项收拢至此,不再阻塞缺陷与需求推进;此前反复出现的阻塞原因是仓库无 package.json、无浏览器测试 harness,无法安全启动真实 Tauri UI。
- 验收: 建立可在测试基座安全启动真实 Tauri UI(或等价 WebView 驱动)的 E2 harness;逐项补齐延期 E2:D-051 桌面权限弹窗真实 UI E2;D-055 切回进程补发 pending ask 前端 E2;D-056 运行中切项目→终态复位 E2;D-060 update/close/reorder 手写内容保留与并发写入回归;D-064 注入故障的 run_task 收尾 E2;D-066 真实 Tauri Window/provider 停止 E2;D-086 runner 级 task→subagent read 拦截执行回归;R-139 bash 硬门禁桌面端真实模型工具调用 E2(2026-08-08 R-139 关闭时转入,验收条款外残余验证);D-202 真机 Event Timing/长任务量化(几百轮会话下 Event Timing 数据 + 侧栏点击 <200ms,2026-08-10 D-241 处置时转入)与 D-202 DOM 节点数上界(对话渲染上限策略,窗口化/折叠历史/分页任一)。
- 拆批(2026-08-08 用户定调「拆出能先做的部分」): **本轮可做**——harness 基座本身(仓库补 package.json、选定并接入 WebView 驱动、安全启动真实 UI、截图与断言框架、失败非零退出),以及不涉及多会话的 E2:D-060 手写内容保留与并发写入、D-086 task→subagent read 拦截、D-064 注入故障的 run_task 收尾、D-066 真实 Window/provider 停止。**留待 R-086**——依赖会话事件路由的三条:D-051 桌面权限弹窗真实 UI、D-055 切回进程补发 pending ask、D-056 运行中切项目终态复位。基座 + 四条 E2 交付即可关闭本条,剩余三条并入 R-086 验收。
- refs: R-086
- 阶段: 3

- 标签: 流程

- 拆批: 2026-08-08 用户定调「拆出能先做的部分」: **本轮可做**——harness 基座本身(仓库补 package.json、选定并接入 WebView 驱动、安全启动真实 UI、截图与断言框架、失败非零退出),以及不涉及多会话的 E2:D-060 手写内容保留与并发写入、D-086 task→subagent read 拦截、D-064 注入故障的 run_task 收尾、D-066 真实 Window/provider 停止。基座 + 四条 E2 交付即可关闭本条;R-086 已于本轮按 §1.2 可用即关闭关闭,原「并入 R-086 验收」的三条桌面 E2(D-051 桌面权限弹窗真实 UI、D-055 切回进程补发 pending ask、D-056 运行中切项目终态复位)留在本条目验收清单执行。

- 进展: 2026-08-16 复核:阻塞解除条件①已满足(node --version/node --check 实测放行,cargo build -p kanzei-app 已启动未拦),当场清空阻塞,恢复推进 B1 基座验证。当前 target/debug/kzapp.exe 构建于 08-12 04:30,早于 CDP 注入提交 695305d(08-12 10:07),需重建后跑 node scripts/e2e-smoke.mjs。
- 状态纠正(2026-08-09): doing→todo。用户已挂起本条,实际不在推进中,却按旧 §1.1 口径占用 doing 名额,与 R-148 一起把 R-153 拒之门外(见 D-219)。恢复推进时再转 doing;挂起前提的小缺陷中 D-185/D-184 仍 open。

- 阻塞: 2026-08-14 分成两半看:①「重建 kzapp」这一半已达成——build-9a06e05 已发布,含 CDP 注入提交 695305d 之后的全部代码,不再是 08-12 的旧构建;②真正卡住的是 D-319——WebView2 Runtime 151 在本机 DevTools 端口不绑定(9 轮实验证据链),e2e-smoke connectOverCDP 20 秒超时,换新构建也绕不过环境问题。另有用户 2026-08-13 的 park 定调(专注 D-318,D-318 现已 fixed 归档)。解除动作: 先解决 D-319(重装/更新 WebView2 Runtime,或拍板改 WebDriver/tauri-driver 路线),再用新版跑 node scripts/e2e-smoke.mjs 验证 B1 基座。解除人: 用户(D-319 那条)。

- 批次: 0/8

## R-135 开发与缺陷修复进度动画显示 [todo]
- 优先级: P0

- 标签: 前端

- 进展: 2026-08-11 扫描:本条仅存标题/优先级/标签,缺 内容/验收/背景——边界不清无法开工(无验收就无从交付,违反 §1.25)。待用户补全条目内容后按序恢复取活,不占可执行槽位。

- 阻塞: 用户: 本条条目内容缺失(仅存标题/优先级/标签,缺 内容/验收/背景),边界不清无法开工(无验收就无从交付,违反 §1.25)。解除动作: 用户补全条目内容(至少验收原文)后按序恢复取活。解除人: 用户。

## R-059 子代理独立升级与移动端通知交互支持 [todo]
- 复杂度: 大
- 优先级: P3
- 原始描述: 手机端可实现子代理和主要代理的交互和通知展示,同时子代理升级为管理项目的容器,可独立于项目存在
- 验收: ①可配置主/子代理间的消息双向通信 ②实时显示来自主要及次级代理的通知推送 ③支持子代理独立升级为管理项目容器(不依赖具体项目结构)
- 已完成: SQLite v2 持久化 agent_notifications 与 delivery_cursors 并有跨重建回放测试(kanzei-core/src/store.rs:496-513/173-256/641-656);运行开始/成功/失败真实写入通知;本机认证 HTTP 桥接已接线(kanzei-app/src/main.rs:1785-1942,回环监听 + bearer 鉴权,提供 health/notifications/messages),设置页有启停按钮;设计文档 docs/design/r059_mobile_agent_communication.md 对边界诚实。
- 退回原因: 2026-08-07 验收核查发现验收三条一条都未实质达成(验收原文要求"在移动端完成")。①双向通信未实现:InMemoryBroker 只被测试使用,生产代码零调用;POST /v1/messages 只把 payload 写成 mobile.message 事件(main.rs:1881),全仓库无任何消费方,消息进库即死信;且该端点因 Content-Length 解析缺陷恒返回 400(见 D-063),从未真正工作过。②移动端实时显示未实现:不存在任何移动端工程,只有本机轮询端点无推送;通知 agent_id 硬编码 "primary"(2532),次级代理从不产生通知。③"子代理升级为项目容器"是空壳:agent_container_*(1944-2013)只往 manifest.json 写字符串,无任何运行时读取,与 SubagentRuntime 零关联,前端"升级到 2"硬编码版本号。
- 下一步: 已完成的属"阶段 B 桌面桥接",应作为独立子需求单独验收;本需求保留移动端三条验收,待用户排期。
- 遗留质量问题: HTTP 桥接与 agent_container 三命令零测试;通知端点要求 thread_id 但无任何端点可枚举 thread,客户端无法自举。
- refs: D-063
- 阶段: 5
- 证据等级: E4
- 设计定位: 功能需求(2026-08-08 用户定调:R-093 的"质量先行"阶段门槛作废,按普通优先级参与取活)

- 标签: 后端

- 进展: 2026-08-08 复核:验收三条原文要求「在移动端完成」,本仓库不存在移动端工程;2026-08-07 退回原因明确本需求保留移动端三条验收、待用户排期。桌面桥接(阶段 B)属既有能力,按退回意见应拆为独立子需求,不在本条验收范围内。
- 阻塞: 用户: 需对移动端三条验收(双向通信/通知推送/子代理升级容器)排期并确认交付载体(真实手机端工程或 web 模拟端)。解除动作:用户拍板移动端交付形态与排期,再按新载体拆子需求动工。

## R-238 大文本交付通道:bash 命令行超长防护 + kz run 文件入口 [todo]
- 内容: ①bash 工具执行前检测命令串长度,超过 Windows 命令行上限(32767 字符,按 30000 留余量判)直接返回结构化错误,不把命令交给 PowerShell 去 spawn 失败;错误文案给出两条正路——大文本先用 write 工具落文件再在命令里引用路径,或改用 ②。②kz CLI `run` 新增 `--prompt-file <path>`:从 UTF-8 文件读取 prompt,与位置参数互斥、可与 --new/--readonly 组合——自举/验收代理喂长材料从此有正门,不必塞 argv。③conventions 增补一条纪律:>8k 字符的文本不进命令行参数,一律文件中转(与①的错误文案同源,一处改两处跟)。
- 来源: 2026-08-14 R-236 验收轮实测:自举代理把约 43 万字符的 prompt 塞进 `cargo run -p kanzei -- run --new <prompt>` 的命令行参数,Windows 32767 上限导致进程 spawn 失败(475ms 退出、PowerShell 异常 5 行、first.out 为空),连续多次同型试错;后续又因 write 工具大内容 JSON 失败绕路。根因是大文本没有交付正门,只能靠代理自己撞出来。Claude 接管联测时用「调小 context_limit + 分轮小消息」绕开了,但坑还在,下一个长输入场景会复发。
- 复杂度: 小
- refs: R-236 D-342
- 标签: 核心
- 验收: ①构造 >32767 字符的 bash 命令,工具返回结构化错误且文案含「文件中转」与「--prompt-file」指引,不发生真实 spawn(单测);②`kz run --prompt-file` 从文件读 prompt 跑通一轮(fake server 集成测试即可),文件不存在/非 UTF-8 有明确报错,与位置参数同给时拒绝;③conventions 文本落地,grep 单一来源;④现有 bash 短命令行为零回归(既有测试全绿)。
- 优先级: P2

## R-239 记忆自动轮采纳率与空轮比例的正常节奏复测(排除 R-226 样本偏置) [todo]
- 优先级: P3
- 内容: R-196 复核发现:修复后(08-12~08-14)自动轮采纳率 2.2%(基线 21.0%)、空轮比例 93.3%(基线 67.6%),两项未改善;但样本高度偏置(R-226 单条线自动推进轮占 22/45,全部 0 fetched),无法区分『修复①无效』与『样本失真』。本条在正常开发节奏(多任务并行、用户轮与自动轮混合)下再测一轮:自动轮 >=50 轮样本,重跑同一组查询对照采纳率与空轮比例,判断修复①检索键切换是否真的无效,还是 R-226 轮次形态造成的假象。
- 复杂度: 小
- 来源: R-196 复核结论(2026-08-16):指标①②未改善,样本偏置需在正常节奏下复测
- 标签: 核心
- 边界: 只做度量与结论,不改实现;若样本仍偏置,继续记录原因,不在本条追修。
- 验收: ①样本为正常开发节奏(非单条线密集自动推进)的自动轮 >=50 轮,写明轮次构成(标题分布/间隔);②重跑与 R-196 同口径的采纳率与空轮比例对照,给出与基线(21.0%/67.6%)和 R-196 修复后值(2.2%/93.3%)的三点对比;③若仍未改善,写明判断原因并指向修复条目标号;若改善,记录修复①实际生效的样本条件。

## R-240 细化运行完成指标统计 [todo]
- priority: P2
- 原始描述: 能更详细的查看各类运行和完成过程种的指标，比如做完不同种类的不同复杂度需求使用的token，方便我们针对上下文和harness等进行优化
- 复杂度: 中
- 归属: kanzei
- 标签: 流程
- 验收: 可按需求类型与复杂度查看运行及完成过程指标，并统计所用 token，支持上下文与 harness 优化分析。

## R-242 会话投影真源切换与分段清空恢复 [todo]
- refs: D-209 D-342 R-236 docs/design/deepseek_harness_upgrade.md
- 依赖: R-241
- 内容: 在 shadow gate 通过后，将 conversation_get/list、runner prior、子代理 transcript 和 UI 历史恢复逐项切到事件投影；进程内 Vec<Message> 仅作缓存。清空对话追加 conversation.reset 并开启新 segment，新 segment 的模型 prior 为空，旧 segment 仍可审计。验证期保留 legacy snapshot 只读对照，五条读路径全部稳定后停止新增 conversation.updated。
- 复杂度: 大
- 批次: 2/4
- 来源: 2026-08-14 DeepSeek Harness 升级方案；用户确认清空保留、删除确定性物理清除并弹窗提示风险。
- 标签: 核心
- 边界: 本需求只负责事件投影真源切换与 segment reset，不实现会话物理删除、Spill artifact 联动删除、WAL/VACUUM 或迁移备份安全整理；这些统一由 R-245 的删除计划与显式整理入口承担。第一批不改事件 format_version 与 SessionFact 公共词表；任一读路径可通过 feature gate 独立回退 legacy snapshot。
- 迁移与回滚: 不新增表、列或索引时不创建空 migration。切换按五条读路径分别启用 feature gate，legacy snapshot 在观察期只读保留；任一路径出现未知差异即回退该路径。全部 gate 稳定后才停止新增 conversation.updated，既有 snapshot 不删除。
- 阻塞: 2026-08-14 前置已满足一半:含 R-241 的安装版本已发布(build-9a06e05,R-241 typed event 真源已 done 并归档;此前装机版停在 08-09,根本产不出 shadow 样本)。剩的是攒样本,不是等决策:装新版后正常使用,直到达到门槛——至少 30 个真实 turn;typed_write_errors 为 0;正常可比较 turn 全部 equal=true;停止、权限拒绝、工具错误、多工具部分完成及受控 draft/tool 重启路径均有可解释证据。解除动作: 用户装新版后正常用几天,样本达标即开工五条读路径真源切换(R-243 随后串行)。解除人: 使用量自然积累(不需要拍板)。
- 验收: ①五条读路径从同一事件日志恢复一致消息；②user/assistant/tool 各安全边界强杀后重启无已发生事实丢失；③孤立 tool call 投影为 interrupted 且不自动重放；④conversation.reset 后新 segment prior 为空但旧 segment 可审计，重复 reset 幂等；⑤至少30个真实 shadow turn 达标，typed_write_errors=0、正常可比较 turn 全部 equal=true、未知差异为0；⑥五条 feature gate 可独立回滚，回滚后 legacy 行为与切换前一致；⑦对照稳定后停止新增 conversation.updated，既有 snapshot 仍可只读回放。
- 优先级: P1

## R-243 Surface Compaction 追加事务：原始事件不变、上下文由 surface 投影 [todo]
- refs: R-236 D-209 docs/design/context_compaction.md docs/design/deepseek_harness_upgrade.md
- 依赖: R-242
- 内容: 将现有 compact_with_digest 的存储语义改为 compaction_started→compaction_summary→surface_replaced→compaction_ended 追加事务；模型上下文只消费 surface projection，原始 Session 事件不修改不删除；连续压缩走已交付滚动合并。
- 复杂度: 中
- 批次: 0/3
- 来源: DeepSeek Harness compaction 事件事务；复用已交付 R-236 的纪要模型、模板和质量闸。
- 标签: 核心
- 边界: 不重写 R-236 纪要算法、压缩模型配置和质量闸；不把 Memory 作为对话恢复源。Compaction 只在 R-242 正式 surface projection 上追加事务，失败保留原 surface，未完成事务在恢复时显式失效；不修改 format_version=1 的既有消息事实。
- 阻塞: 等待 R-242 完成五条读路径真源切换并冻结正式 SessionProjection/segment 语义；R-243 与 R-242 由同一主线串行实施，不交给并行自举线。
- 验收: ①压缩前后 raw event hash 不变；②边界上的 tool call/result 必须完整配对，否则拒绝压缩；③不完整 compaction transaction 重启后不生效且有可见诊断；④连续两次压缩 replay 一致，首段关键实体仍保留；⑤模型 surface 变短但 transcript/audit 仍能回看原文；⑥R-236 全部压缩回归保持通过。
- 优先级: P1

## R-244 统一 Tool Pipeline：Policy、单调 Guard、Wrapper、Result Policy 与 Observer [todo]
- refs: D-209 R-180 R-174 docs/design/deepseek_harness_upgrade.md
- 内容: 在 kanzei-harness 建立固定工具阶段 parse/materialize→policy allow/deny/ask→monotonic guards→execution wrappers→tool body→result policies→immutable observers；复用现有 Ruleset 普通规则、hard_denies、managed fence、timeout、progress、cancellation、recall 与 trace，不重写规则引擎。
- 前置: R-241
- 复杂度: 大
- 批次: 0/5
- 来源: DeepSeek Harness tool execution pipeline 对照；Kanzei 当前阶段散落在 drive.rs 和工具内部。
- 标签: 核心
- 边界: 现有权限行为必须逐条保持；hard deny、托管文件与 writer ownership 属不可逆 Guard，后续 hook 不得放宽。Observer 只能观察最终结果，不得修改 ToolOutput 或反向影响执行。第一批仅迁移一个无副作用工具验证流水线，再分族迁移。
- 阻塞: 
- 验收: ①每阶段有独立契约测试且顺序固定；②现有 Ruleset/hard_denies 回归逐字节一致；③policy allow 不能覆盖 Guard deny，有反证测试；④timeout/cancellation/progress 只在 wrapper 实现一处；⑤observer 抛错不改变工具事实终态但留下遥测；⑥至少 read/bash/git/子代理工具走统一通道且无双执行；⑦失败、拒绝、取消路径都产生唯一 final result。
- 优先级: P1
- 进展: 2026-08-14 用户定调:R-244 列入主任务,由主线串行实施(与 R-242/R-243 同一条线,不拆给并行自举线)。另一半前提「与 R-241 的事件类型需先冻结」也已达成——R-241 已 done 并归档,typed event 真源冻结。两条阻塞前提均消失,阻塞字段清空,按 P1 入队。
- observed_head: 96313679e027a6ca76aa2003e85a46cc0109bb80
- observed_worktree_hash: fnv1a64:cbf29ce484222325
- recorded_at: 1786712688355

## R-245 Tool Result Spill 与显式空间整理：完整 artifact、可恢复引用、无自动过期 [todo]
- refs: D-209 R-180 D-297 D-298 R-242 docs/design/deepseek_harness_upgrade.md
- 依赖: R-242 R-244
- 内容: 统一工具结果为 Inline 或 Spilled{preview,artifact_id,bytes,sha256,retrieval_hint}；read 优先指向原文件 offset/limit，bash、git、test_record、web 等完整原文进入与 state.db 同生命周期的 Git 忽略运行目录。提供存储与整理入口，按类别、会话、日期、大小预览占用，支持清理无引用 artifact；经风险确认后，用可恢复失败的删除计划物理删除已选会话的事件、投影和引用 artifact；并支持 SQLite checkpoint、VACUUM 与迁移备份管理。默认不自动过期。
- 复杂度: 大
- 批次: 0/5
- 来源: DeepSeek Harness spill policy、本地 state.db 输出分布统计，以及用户确认“不自动过期但需要显式整理入口”。
- 标签: 核心
- 边界: 任何事件仍引用的 artifact 不得被静默清理；整理前显示预计释放空间和不可恢复范围，执行后给清单与实际释放量。32 KiB 先做 shadow telemetry。普通会话删除保证产品不可检索且重启不复生；安全整理才处理 SQLite freelist、WAL 和含旧正文备份。当前库为 WAL、secure_delete=OFF、auto_vacuum=NONE，不能把 DELETE 行等同磁盘字节已擦除。弹窗必须区分仅删除与删除并安全整理，取消零写入；显式整理不是定时任务。
- 迁移与回滚: artifact 原子写入后再提交引用事件；任一步失败不得留下有效事件指向缺失文件。删除使用引用图和事务清单，失败可重试；schema 迁移前备份。关闭 Spill 可回到 Inline，但已有引用仍必须可读。
- 阻塞: 未完成依赖 R-242、R-244(依赖式阻塞,非人工 park)。原文里「R-244 是否列入主任务待用户决定」这一条已在 2026-08-14 由用户拍板:R-244 列入主任务、主线串行做。故本条剩纯依赖:等 R-242 固定 segment/会话投影边界、等 R-244 冻结 Result Policy 与 ToolOutput 公共契约;契约冻结后 telemetry、artifact 适配和整理 UI 可拆给自举线,物理删除与安全整理事务仍由主线审查。解除人: 依赖自然解除(R-242 + R-244 完成即解)。
- 验收: ①32 KiB shadow telemetry 不改变模型输入并产出按工具分布；②Spill 原文 sha256 与工具原输出一致，重启后可取回；③事件提交与 artifact 写入故障注入无悬空引用；④明确无自动过期任务；⑤整理入口列出总占用、数据库、WAL、freelist、artifact、无引用文件和迁移备份并支持 dry-run；⑥清理引用中 artifact 被拒，清理无引用 artifact 成功且释放量可核对；⑦删除弹窗列出会话事件、轨迹、草稿与 artifact，仅删除和删除并安全整理差异明确，取消零写入；⑧确认删除后事件、投影和引用 artifact 产品层不可检索且重启不复生，删除计划任一点失败可恢复重试；⑨安全整理仅在运行静止时执行，成功后 checkpoint、VACUUM 与备份处置可核对，busy 或失败不静默；⑩权限、路径逃逸、不可预测文件名和磁盘配额有测试。
- 优先级: P1

## R-246 LineRuntime 统一资源 owner：幂等 dispose 与持久服务显式移交 [todo]
- refs: R-174 R-175 R-180 D-275 docs/design/session_state_and_line_runtime.md docs/design/deepseek_harness_upgrade.md
- 内容: 建立 LineRuntime，统一持有 cancellation token、active run、child agents、transcript projection、background results、notifications、background processes、writer/read leases、worktree binding 和 temporary artifacts。dispose 幂等且并发调用共享同一完成 future；persistent 服务必须通过 adoption 事件显式移交 ProjectRuntime。
- 前置: R-241 R-244
- 复杂度: 大
- 批次: 0/5
- 来源: DeepSeek Harness Scope 生命周期约束；Kanzei 已有 cancellation、子代理、transcript、notification、background process 多注册表。
- 标签: 核心
- 边界: 不重做 R-180 已交付的长驻服务注册表和日志；以适配/收口方式接入。普通资源生命周期不超过 LineRuntime；persistent 只能显式 adopt，不接受布尔值或 drop 泄漏式脱离 owner。
- 阻塞: 依赖 R-244 的 Tool Pipeline 契约冻结(R-244 已于 2026-08-14 由用户定为主线串行的主任务,不再是「是否列入」的悬案)。本条自己那半——是否交给自举线实施——仍待用户定,但不必现在定:R-244 落地前本条无论如何开不了工。解除人: 依赖自然解除后由用户决定实施线路。
- 验收: ①并发两次 dispose 共享完成结果且只收尾一次；②取消子代理并等待退出，三种终态均释放读槽；③非 persistent 后台进程、通知订阅、临时 artifact 和租约全部回收；④dispose 返回前工具 wrapper 已静止且生命周期终态落库；⑤persistent 服务显式 adopt 后跨 run 存活并有 adoption 事件，未 adopt 的全部收回；⑥强杀重启后无幽灵 owner，能确定恢复或标失败；⑦R-174/R-180 现有测试保持通过。
- 优先级: P2

## R-248 先行调研内建:新方向开工前默认产出「已有方案对照」,不靠用户开口 [todo]
- refs: R-221 docs/design/research_mode.md
- 依赖: R-221
- 内容: 把「先查已有方案再动手」从用户每次口头要求变成 harness 的默认动作。①触发判据机械可判、不交模型自由裁量:项目根首次初始化 `.kanzei/`、req add 时 refs 为空且标签为核心、用户显式发起,三者之一成立即触发;②产物落 `.kanzei/research/<topic>/prior-art.md`,每条结论含「方案名 + 出处(URL 或 file:line) + 与本课题的差异 + 采用或不采用的理由」,**外部已有实现**(开源方案、协议、公开设计)与**仓内既有设计**(docs/design/**、requirements/defects 现存与 archive)两侧都必须覆盖;③新方向判定成立而无对照工件时,req add 要求 refs 指向该工件,或由用户显式豁免并留痕。
- 复杂度: 中
- 批次: 0/3
- 来源: 2026-08-14 用户观察——开新项目应先深度调研已有方案与设计,不适合从零开始;这是当前 coding agent 的通病(非得用户主动请求才去调研),直接影响自举质量。
- 标签: 核心
- 边界: 不是每条需求都调研,只在触发判据成立时启动;判据必须机械可判,不接受模型自行裁量「这算不算新方向」。websearch 轮次设上限,不做无限扩散爬取。本条只产出对照工件与开工门禁,不改 req/defect 状态机,也不自动把调研结论写成条目——那是 R-221 定调点4 的回流通道。
- 阻塞: 2026-08-14 用户定调已解决定位问题——research 是独立深度研究模式,主形态即「文献 + 仓库的深度调研」,网络检索是主力;本条的先行调研正是该形态的一种应用,归属明确,原「网络检索被降级为辅助」的矛盾消失。用户同时定调**本条不拆批**(仓内既有设计对照那半也不提前落 dev),整条跟随 R-221 排在 dev 稳定之后。故阻塞= R-221(排期 + §1 重写后重推定调点),解除人:用户。
- 验收: ①三种触发判据各有定向测试,未触发的普通条目不受影响;②prior-art.md 每条结论都带出处,无出处结论被机械拒绝(复用 V0 标注同一套校验);③外部与仓内两侧覆盖各有独立断言,只查一侧不算通过;④新方向下 req add 缺 refs 被拒,豁免路径留痕可审计;⑤websearch 轮次上限有实测,超限给明确诊断而非静默截断;⑥既有 req add 路径无回归。
- 优先级: P1

## R-249 工具结果可返回图片:ToolOutput 承载 image part,打通图片读取与 UI 截图 [doing]
- refs: R-014 R-101 R-244 R-245
- 依赖: R-244
- 内容: 现状 `ToolOutput.content` 只有 String(kanzei-harness/src/tool.rs:178),任何工具都无法把图片交给模型;`Part::Image` 的三协议映射早在 R-014 交付,但入口只有桌面端用户附件(kanzei-app/src/state.rs:29)。本条把 ToolOutput 扩成可携带 image part,并打通两个消费点:①`read` 读图片文件(PNG/JPEG/WebP/GIF)按 media_type 编码返回;②UI 自检补截图通道——现有 `ui_probe` 窗口通道加 `screenshot`,让 ui_dom/ui_style 的结构读数配上真实渲染画面。
- 复杂度: 大
- 批次: 0/4
- 来源: 2026-08-14 三系统工具面对照(DeepSeek harness / Claude Code / kanzei):read_image 是唯一的能力硬缺口。桌面端 ui_dom/ui_console/ui_style 能读结构与数值但看不见渲染结果,对齐、遮挡、观感一类问题无法自查。
- 标签: 核心
- 边界: ToolOutput 是 harness 核心契约,R-244 明确要冻结「ToolOutput 公共契约」、R-245 要把它改成 Inline/Spilled 二态——本条**不得抢在 R-244 之前改这个结构**,否则必然返工。图片体积走 R-245 的 spill 口径,不在 ToolOutput 内联大 base64。不实现 UI 点击/输入/滚动(那是 R-101 的 E2 harness 范围),本条只做「看得见」不做「动得了」。deepseek_responses 协议当前丢弃 Image part,本条不负责补齐该 provider,但要在 provider 不支持时给出显式降级提示,不静默丢弃。
- 进展: 2026-08-14 批1 交付(1831239)。勘察修正了原条目的一处前提:`Part::Image` 的三协议映射早在 R-014 就通了,缺的只是**工具侧出口**,协议层零改动即可打通——不必等 R-244。实现:①ToolOutput 增 images 载荷(空 vec 与既有行为逐字节一致,53 处 `ToolOutput {` 里只有 4 个真构造点,其余是解构模式);②read 按 magic bytes 而非扩展名识图(PNG/JPEG/WebP/GIF),扩展名撒谎会让 media_type 与真实字节不符、provider 400 且报错指向请求体;③图片 Part 只能追加在所有 ToolResult 之后——Anthropic 要求 tool_result 块在 user 消息最前,而 results[i]↔calls[i] 由 note_step 的 debug_assert 锁着,中间也不能插;④provider 不支持时**在进 messages 前**降级为显式文本说明,判据收敛为 Route::supports_images() 与 client.rs 硬拒绝共用一处。新增 10 条测试。**剩余批次**:批3 图片 artifact 走 R-245 spill(仍等 R-244/R-245);批4 deepseek 协议补齐(不在本条范围,只保证降级不静默)。 || 2026-08-14 批2 交付:新增 ui_screenshot 工具(kanzei-app/src/screenshot.rs)。实窗验证三轮才对,两次假绿都值得记——①未声明 DPI 感知时 GetWindowRect 返回虚拟化坐标(2582px 的窗口报成 1295px),抓到的是横跨多个窗口的错误区域;②改用正确矩形后,屏幕 DC 抓取拿到的是压在上面那个应用的界面(kzapp 被完全遮挡),内容丰富所以 looks_blank 一路放行。两次都是「测试通过但抓的不是那个窗口」。最终改用 PrintWindow+PW_RENDERFULLCONTENT 离屏渲染,免疫遮挡,在完全被盖住的状态下抓到 kzapp 完整界面并经人眼与用户实拍逐项比对一致;屏幕 DC 仅在 PrintWindow 失效且本窗口为前台时作回退,不是前台宁可报错——返回别人的界面比返回错误坏得多。测试记录 T-1786705800。
- 阻塞: 批1 已解除(协议层无需改动,不依赖 R-244)。批2 无阻塞、随时可开工——本条目前挂 doing 且被阻塞字段整条 park,调度看不到批2;R-202 关闭腾出 WIP 槽后应直接续做批2(不需要用户拍板)。批3 仍等 R-244 冻结 ToolOutput 公共契约与 Result Policy、R-245 确定图片类 artifact 的 spill 落点——原文「R-244 是否列入主任务待用户决定」已于 2026-08-14 拍板:列入主任务、主线串行做,故批3 是纯依赖等待。解除人: agent(批2)/ 依赖自然解除(批3)。
- 验收: ①read 读 PNG/JPEG/WebP/GIF 各有定向测试,media_type 正确,非图片文件走原文本路径无回归;②ui_probe screenshot 返回的图片能被模型消费,桌面端实测有轨迹;③provider 不支持图片时有显式降级诊断;④图片 artifact 走 R-245 spill,ToolOutput 不内联超阈值 base64;⑤R-014 既有附件路径逐条无回归;⑥ToolOutput 结构变更后既有全部工具返回路径编译通过且行为不变(机械核验)。
- 优先级: P1

## R-251 试用手册配置移至设置模块 [todo]
- 复杂度: 小
- 标签: 流程
- 验收: 试验相关配置已迁移至主界面→设置→高级功能区域
- 优先级: P2

## R-252 目标区改造成原始想法收件箱:新建 IDEAS 文档线、退役 goal、拆解由人点触发子代理 [todo]
- 内容: 把「目标」区改造成用户侧的原始想法收件箱:录入未经拆解的设计需求/想法,再由人点一下派子代理拆成 R-xxx / D-xxx。①新建 IDEAS 文档线(前缀 I,状态 inbox/split/dropped),不复用 GOALS 换语义——goals 线同批退役(现存 G-001~G-003 推 dropped 并归档);②录入不过模型,原样收下(用户想法的原话就是最有价值的部分,过一遍 fast 模型只会磨平);③拆解由人点按钮派子代理(idea_split 命令,照 quick_req 的模式:写租约 + 组件挂 req/defect/idea + before/after 差集取真实新增 ID),不做自动拆解;④转 split 时硬门禁:refs 必须非空且每个 ID 在 requirements/defects 的活跃或归档里真实存在,否则「已拆解」就是一句空话;⑤想法只把计数与标题注入 agent 每轮上下文,不注全文(避免未拆解的想法污染取活)。
- 备注: 本条与其余九条一起勘察,唯独它需要动 13 个文件,其中 crates/kanzei-app/src/run.rs 的动作表有一行 goal→idea——那是与后端自举线唯一抢文件的地方。用户 2026-08-14 拍板:其余九条本轮做完发版,本条另登需求进队列,等 R-202 收尾后单独做。完整勘察(文件锚点/DOM/状态机/门禁设计)见会话记录。
- 复杂度: 大
- 来源: 2026-08-14 用户提的十条前端改造之六。原话:目标现在似乎没用?目标区可以改成我们用户侧输入的一些比较原始的设计需求想法,也就是待拆解成需求和缺陷的源。勘察证实 goals 线确实零消费者:取活引擎(work.rs)不看目标,鞭挞的推进指令(auto_run.rs)只点名 requirements.md/defects.md,前端除了渲染三条也没有别的用途。
- 标签: 核心
- 验收: ①IDEAS 文档线可增删改查,状态机 inbox→split/dropped 有测试;②goal 线退役:现存三条推 dropped 并归档,tracker/CLI/前端/managed_fence/记忆控制平面里的 goal 全部改指 idea,全仓 grep 零残留;③转 split 的 refs 硬门禁有正反测试(refs 空拒、指向不存在的 ID 拒、指向归档条目放行);④前端:侧栏「目标」区换成「想法」,有录入入口与「拆解」按钮,拆解后显示产出的 R/D 编号;⑤idea_split 子代理跑通一次真实拆解(fake server 集成测试即可);⑥取活引擎不看想法(work.rs 不动),鞭挞的推进指令也不点名想法队列——想法不是待办。
- 优先级: P2
